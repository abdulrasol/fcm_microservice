import sys

file_path = "/Users/rasol/DevsTools/codes/rust/fcm_microservice/src/main.rs"

with open(file_path, "r") as f:
    c = f.read()

c = c.replace(
    'Json(payload): Json<SendNotificationRequest>,',
    'Json(mut payload): Json<SendNotificationRequest>,'
)

c = c.replace(
    'if payload.topic.is_none() && payload.token.is_none() && payload.condition.is_none() {',
    'payload.app = payload.app.trim_end_matches(".json").to_string();\n\n    if payload.topic.is_none() && payload.token.is_none() && payload.condition.is_none() {'
)

c = c.replace(
    'message["topic"] = json!(topic);\n        // Automatically save topic to DB\n        let _ = db::save_topic(&state.db, &payload.app, topic).await;',
    'let clean_topic = topic.trim_start_matches("/topics/");\n        message["topic"] = json!(clean_topic);\n        // Automatically save topic to DB\n        let _ = db::save_topic(&state.db, &payload.app, clean_topic).await;'
)

with open(file_path, "w") as f:
    f.write(c)
