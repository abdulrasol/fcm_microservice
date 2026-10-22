#!/usr/bin/env bash
# Deploys the service on the home server with a backup first and a way back.
#
#   deploy/deploy.sh deploy [--yes]       back up, update the code, rebuild, start, smoke-test
#   deploy/deploy.sh rollback <backup>    restore a backup made by `deploy` (a path under backups/)
#   deploy/deploy.sh create-owner <email> create the first dashboard owner (the password is printed once)
#   deploy/deploy.sh status               container, health, commit, recent log lines
#
# First run (the script is not on the server yet):
#   git fetch origin && git show origin/my-server:deploy/deploy.sh > ~/deploy.sh && chmod +x ~/deploy.sh
#   FCM_DIR=/path/to/fcm_microservice ~/deploy.sh deploy
#
# Run it from anywhere on the server; it moves to the repository root itself.
# It never pushes, never prints the contents of .env or of the credentials.
set -euo pipefail

# FCM_DIR lets the very first run use a copy of this script from outside the
# repository (the script is not on the server until the first update).
cd "${FCM_DIR:-$(dirname "$0")/..}"
ROOT="$(pwd)"
[ -f docker-compose.yml ] || { echo "ERROR: $ROOT is not the repository root (set FCM_DIR)" >&2; exit 1; }
BRANCH="${DEPLOY_BRANCH:-my-server}"
HOST_PORT="${HOST_PORT:-8084}"
CONTAINER=fcm_microservice
SUDO=""
if [ "$(id -u)" -ne 0 ]; then SUDO="sudo"; fi

say()  { printf '\n== %s\n' "$*"; }
die()  { printf 'ERROR: %s\n' "$*" >&2; exit 1; }
ask()  { # ask "question"  -> returns 0 on yes
  if [ "${ASSUME_YES:-0}" = "1" ]; then return 0; fi
  read -r -p "$1 [y/N] " reply
  [[ "$reply" =~ ^[Yy]$ ]]
}

need() { command -v "$1" >/dev/null 2>&1 || die "'$1' is required. Debian: sudo apt-get install -y $2"; }

preflight() {
  need docker docker.io
  need git git
  need curl curl
  need sqlite3 sqlite3
  docker compose version >/dev/null 2>&1 || die "docker compose is not available"
  [ -f .env ] || die ".env is missing. Copy .env.example to .env and fill it in first."
  grep -Eq '^CORS_ALLOWED_ORIGINS=.+' .env || die "CORS_ALLOWED_ORIGINS is empty in .env (the service refuses to start without it)"
  if ! grep -Eq '^COOKIE_SECURE=false' .env; then :; else
    echo "WARNING: COOKIE_SECURE=false. Use it only for plain-HTTP tests, never in production."
  fi
  if ! grep -Eq '^TRUST_PROXY=true' .env; then
    echo "NOTE: TRUST_PROXY is not true. Behind Caddy it should be true (client IPs and rate limits)."
  fi
}

# Copies the database (plus its WAL files) of the current layout into $1,
# then checkpoints the copy so that it is one consistent file.
backup_database() {
  local dest="$1"
  mkdir -p "$dest"
  local found=0
  if $SUDO test -f data/data.db; then   # new layout: a directory
    for f in data.db data.db-wal data.db-shm; do
      if $SUDO test -f "data/$f"; then $SUDO cp "data/$f" "$dest/$f"; found=1; fi
    done
  fi
  if [ "$found" = 0 ] && [ -f data.db ]; then   # old layout: a single mounted file
    cp data.db "$dest/data.db"; found=1
    # In the old layout the -wal/-shm files live inside the stopped container.
    for f in data.db-wal data.db-shm; do
      docker cp "$CONTAINER:/app/$f" "$dest/$f" 2>/dev/null || true
    done
  fi
  [ "$found" = 1 ] || die "no database found (looked for data/data.db and data.db)"
  $SUDO chown -R "$(id -u):$(id -g)" "$dest"
  sqlite3 "$dest/data.db" "PRAGMA wal_checkpoint(TRUNCATE);" >/dev/null
  local result
  result="$(sqlite3 "$dest/data.db" "PRAGMA integrity_check;")"
  [ "$result" = "ok" ] || die "the backup copy of the database failed integrity_check: $result"
  rm -f "$dest/data.db-wal" "$dest/data.db-shm"
  echo "database backup OK ($(du -h "$dest/data.db" | cut -f1))"
}

wait_healthy() {
  for _ in $(seq 1 60); do
    if curl -fsS --max-time 3 "http://127.0.0.1:${HOST_PORT}/health" >/dev/null 2>&1; then return 0; fi
    sleep 1
  done
  return 1
}

cmd_deploy() {
  [ "${1:-}" = "--yes" ] && ASSUME_YES=1
  preflight

  say "Checking the repository"
  [ "$(git rev-parse --abbrev-ref HEAD)" = "$BRANCH" ] || die "you are on '$(git rev-parse --abbrev-ref HEAD)', expected '$BRANCH' (set DEPLOY_BRANCH to override)"
  [ -z "$(git status --porcelain --untracked-files=no)" ] || die "there are local changes to tracked files; commit or stash them first"
  git fetch origin
  local before after
  before="$(git rev-parse HEAD)"
  after="$(git rev-parse "origin/$BRANCH")"
  git merge-base --is-ancestor "$before" "$after" || die "origin/$BRANCH is not a fast-forward of the local branch"
  echo "current : $(git log -1 --oneline "$before")"
  echo "target  : $(git log -1 --oneline "$after")"
  echo "commits to apply: $(git rev-list --count "$before..$after")"
  ask "Continue with the deployment?" || die "cancelled"

  local stamp backup
  stamp="$(date +%Y%m%d-%H%M%S)"
  backup="$ROOT/backups/$stamp"
  mkdir -p "$backup"
  chmod 700 "$ROOT/backups" "$backup"

  say "Stopping the service gracefully (the WAL is checkpointed)"
  docker compose stop -t 30 || true

  say "Backing up to $backup"
  echo "$before" > "$backup/HEAD.txt"
  cp .env "$backup/env.backup" && chmod 600 "$backup/env.backup"
  if [ -d credentials ]; then $SUDO tar -czf "$backup/credentials.tgz" credentials && $SUDO chown "$(id -u):$(id -g)" "$backup/credentials.tgz" && chmod 600 "$backup/credentials.tgz"; fi
  backup_database "$backup/db"

  say "Preparing the data directory"
  if ! $SUDO test -f data/data.db; then
    mkdir -p data
    cp "$backup/db/data.db" data/data.db
    echo "copied the checkpointed database into data/ (the old ./data.db is left untouched)"
  else
    echo "data/data.db already exists; keeping it"
  fi

  say "Updating the code"
  git merge --ff-only "origin/$BRANCH"

  say "Building and starting"
  docker compose build
  docker compose up -d

  say "Waiting for /health"
  if ! wait_healthy; then
    docker compose logs --tail 60 "$CONTAINER" || true
    die "the service did not become healthy. Roll back with: deploy/deploy.sh rollback $backup"
  fi

  say "Smoke tests"
  if ! HOST_PORT="$HOST_PORT" deploy/smoke.sh; then
    die "smoke tests failed. Roll back with: deploy/deploy.sh rollback $backup"
  fi

  say "Done"
  echo "Backup : $backup"
  echo "Commit : $(git log -1 --oneline)"
  echo "Next   : deploy/deploy.sh create-owner you@example.com   (first time only)"
}

cmd_rollback() {
  local backup="${1:-}"
  [ -n "$backup" ] || die "usage: deploy/deploy.sh rollback backups/<stamp>"
  backup="$(cd "$backup" && pwd)" || die "no such backup directory"
  [ -f "$backup/HEAD.txt" ] && [ -f "$backup/db/data.db" ] || die "'$backup' is not a backup made by this script"
  local old_head
  old_head="$(cat "$backup/HEAD.txt")"
  echo "This restores commit $old_head, the database, .env and credentials from $backup."
  echo "Everything written to the service after that backup is lost."
  ask "Roll back?" || die "cancelled"

  docker compose stop -t 30 || true
  if [ -d data ]; then $SUDO mv data "data.rolledback-$(date +%Y%m%d-%H%M%S)"; fi
  git checkout -B "$BRANCH" "$old_head"
  cp "$backup/db/data.db" ./data.db      # the layout of the old commit
  cp "$backup/env.backup" .env && chmod 600 .env
  if [ -f "$backup/credentials.tgz" ]; then $SUDO rm -rf credentials && $SUDO tar -xzf "$backup/credentials.tgz"; fi
  docker compose build
  docker compose up -d
  wait_healthy || die "the old version did not become healthy; check: docker compose logs"
  echo "Rolled back to $(git log -1 --oneline)"
}

cmd_create_owner() {
  local email="${1:-}"
  [ -n "$email" ] || die "usage: deploy/deploy.sh create-owner you@example.com"
  echo "A temporary password is printed once. Copy it now, sign in, and change it."
  echo "Do not paste it anywhere (chats included)."
  docker compose exec "$CONTAINER" fcm_microservice create-owner --email "$email"
}

cmd_status() {
  docker compose ps
  curl -fsS --max-time 3 "http://127.0.0.1:${HOST_PORT}/health" && echo " <- /health"
  git log -1 --oneline
  docker compose logs --tail 20 "$CONTAINER"
}

case "${1:-}" in
  deploy)       shift; cmd_deploy "$@" ;;
  rollback)     shift; cmd_rollback "$@" ;;
  create-owner) shift; cmd_create_owner "$@" ;;
  status)       cmd_status ;;
  *) sed -n '2,16p' "$0"; exit 2 ;;
esac
