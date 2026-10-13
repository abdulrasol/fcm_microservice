use serde_json::json;
fn main() {
    let mut message = json!({
        "notification": {
            "title": "foo",
        }
    });
    message["topic"] = json!("all");
    println!("{}", message);
}
