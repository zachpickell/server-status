//! Sends alerts to configured notification channels.

use crate::config::Discord;
use crate::poller;

pub fn send_discord_alert(discord: &Discord, message: &str) -> Result<(), String> {
    let body = serde_json::json!({ "content": message }).to_string();
    poller::new_agent()
        .post(&discord.webhook)
        .header("Content-Type", "application/json")
        .send(body)
        .map_err(|e| format!("failed to send webhook: {e}"))?;
    Ok(())
}
