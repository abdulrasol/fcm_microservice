# FCM Microservice & Dashboard

A fully integrated, ultra-lightweight Push Notification Management System built in Rust. It serves as both a high-performance REST API for sending Firebase Cloud Messaging (FCM) notifications to multiple projects and a web-based dashboard.

## Features

- **Multi-App Support**: Send notifications to multiple Firebase projects dynamically by dropping their `service-account.json` into the `credentials/` folder.
- **Single Docker Container**: The Rust server handles both the REST API and serves the static Web Dashboard on a single port.
- **Ultra Lightweight**: Built with `axum` and `tokio`. Minimal RAM and CPU consumption.
- **Integrated SQLite Database**: Automatically tracks notification history and saved topics.
- **Hot-Reloading Credentials**: Automatically detects new credentials added to the folder without a server restart.
- **Web Dashboard**: An integrated UI to manually send notifications and view history.

## Documentation

For a comprehensive guide on how to interact with the API, required payloads, and responses, please refer to the [API Documentation](docs.md).

## Getting Started

1. Clone the repository.
2. Copy `.env.example` to `.env` and fill in your details.
3. Place your Firebase service account JSON files inside the `credentials/` directory.
   - Example: `credentials/create_app.json`
4. Run the server using Docker or `cargo run`.

## Docker Deployment
```bash
docker build -t fcm-microservice .
docker run -d -p 8080:8080 -v $(pwd)/credentials:/app/credentials -v $(pwd)/data.db:/app/data.db --env-file .env fcm-microservice
```
