// The routes of the dashboard.

import { createRouter, createWebHistory } from 'vue-router'
import { canAccess, isOwner, load, session } from './session'
import { decideRoute, loginRedirectLoopGuard, resetLoginRedirectGuard } from './lib/guard'
import { resetRedirectFlag, setRouter } from './api'
import Login from './views/Login.vue'
import ChangePassword from './views/ChangePassword.vue'
import Apps from './views/Apps.vue'
import AddApp from './views/AddApp.vue'
import AppSettings from './views/AppSettings.vue'
import Users from './views/Users.vue'
import Audit from './views/Audit.vue'
import Account from './views/Account.vue'
import NotFound from './views/NotFound.vue'

const routes = [
  { path: '/login', name: 'login', component: Login },
  { path: '/password', name: 'password', component: ChangePassword, meta: { requiresAuth: true } },
  { path: '/', name: 'apps', component: Apps, meta: { requiresAuth: true } },
  { path: '/apps/new', name: 'app-new', component: AddApp, meta: { requiresAuth: true, owner: true } },
  { path: '/apps/:app', name: 'app-settings', component: AppSettings, meta: { requiresAuth: true } },
  { path: '/users', name: 'users', component: Users, meta: { requiresAuth: true, owner: true } },
  { path: '/audit', name: 'audit', component: Audit, meta: { requiresAuth: true } },
  { path: '/account', name: 'account', component: Account, meta: { requiresAuth: true } },
  { path: '/:pathMatch(.*)*', name: 'not-found', component: NotFound, meta: { requiresAuth: true } },
]

const router = createRouter({
  history: createWebHistory(),
  routes,
})

// Wire router to api so 401 interceptor uses router.replace instead of full page reload
setRouter(router)

router.beforeEach(async (to) => {
  if (!session.loaded) {
    await load()
  }
  const decision = decideRoute({
    to,
    me: session.me,
    owner: isOwner.value,
    canAccess,
  })
  if (!decision) {
    return true
  }
  if (decision.redirect && decision.redirect.startsWith('/login')) {
    if (!loginRedirectLoopGuard()) {
      return to.path === '/login' ? true : false
    }
  }
  if (decision.message) {
    router.state.__message = decision.message
  }
  return { path: decision.redirect }
})

router.afterEach(() => {
  resetLoginRedirectGuard()
  resetRedirectFlag()
})

export default router
