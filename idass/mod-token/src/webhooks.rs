use serde_json::Value;
use std::time::Duration;
use tracing::warn;

/// Posts `current_claims` to `webhook_url` and merges the JSON response into the claims.
/// On timeout (>150ms) or any error, returns `current_claims` unchanged (fail-open).
pub async fn enrich_claims(client: &reqwest::Client, webhook_url: &str, current_claims: Value) -> Value {
    let result = tokio::time::timeout(
        Duration::from_millis(150),
        call_webhook(client, webhook_url, &current_claims),
    )
    .await;

    match result {
        Ok(Ok(extra)) => merge_claims(current_claims, extra),
        Ok(Err(e)) => {
            warn!(error = %e, url = webhook_url, "webhook error — using default claims");
            current_claims
        }
        Err(_) => {
            warn!(url = webhook_url, "webhook timeout after 150ms — using default claims");
            current_claims
        }
    }
}

async fn call_webhook(client: &reqwest::Client, url: &str, claims: &Value) -> Result<Value, reqwest::Error> {
    let resp = client.post(url).json(claims).send().await?;
    resp.json::<Value>().await
}

fn merge_claims(mut base: Value, extra: Value) -> Value {
    if let (Some(base_obj), Some(extra_obj)) = (base.as_object_mut(), extra.as_object()) {
        for (k, v) in extra_obj {
            base_obj.insert(k.clone(), v.clone());
        }
    }
    base
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;
    use wiremock::{matchers::{method, path}, Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn successful_webhook_enriches_claims() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/enrich"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"role": "admin"})))
            .mount(&server).await;

        let result = enrich_claims(&reqwest::Client::new(), &format!("{}/enrich", server.uri()),
            json!({"sub": "u1", "email": "u@test.com"})).await;
        assert_eq!(result["role"], "admin");
        assert_eq!(result["sub"], "u1");
    }

    #[tokio::test]
    async fn timeout_returns_original_claims() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/slow"))
            .respond_with(ResponseTemplate::new(200)
                .set_body_json(json!({"role": "admin"}))
                .set_delay(Duration::from_millis(300)))
            .mount(&server).await;

        let base = json!({"sub": "u1"});
        let result = enrich_claims(&reqwest::Client::new(), &format!("{}/slow", server.uri()), base.clone()).await;
        assert_eq!(result, base);
    }

    #[tokio::test]
    async fn server_error_returns_original_claims() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/err"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server).await;

        let base = json!({"sub": "u2"});
        let result = enrich_claims(&reqwest::Client::new(), &format!("{}/err", server.uri()), base.clone()).await;
        assert_eq!(result, base);
    }

    #[test]
    fn merge_claims_combines_objects() {
        let base = json!({"sub": "u1", "email": "u@test.com"});
        let extra = json!({"role": "admin", "org": "acme"});
        let merged = merge_claims(base, extra);
        assert_eq!(merged["role"], "admin");
        assert_eq!(merged["sub"], "u1");
    }
}
