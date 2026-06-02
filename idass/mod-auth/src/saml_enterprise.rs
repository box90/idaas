use base64::{engine::general_purpose::STANDARD as B64, Engine};
use flate2::{write::DeflateEncoder, Compression};
use serde::{Deserialize, Serialize};
use shared_kernel::{crypto, error::AppError, models::{Connection, User}};
use sqlx::PgPool;
use std::io::Write;
use uuid::Uuid;

#[derive(Deserialize, Serialize)]
pub struct SamlConnectionOptions {
    pub idp_sso_url: String,
    pub idp_entity_id: String,
    pub idp_certificate_pem: String,
    pub sp_entity_id: String,
    pub acs_url: String,
}

/// Builds a redirect URL containing an unsigned SAML AuthnRequest.
pub fn generate_authn_request(
    connection: &Connection,
    master_key: &[u8; 32],
) -> Result<String, AppError> {
    let decrypted = crypto::decrypt(master_key, &connection.options_encrypted)?;
    let opts: SamlConnectionOptions =
        serde_json::from_slice(&decrypted).map_err(|_| AppError::InvalidConnection)?;

    let xml = format!(
        r#"<samlp:AuthnRequest xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol"
            ID="id_{id}" Version="2.0" IssueInstant="{ts}"
            Destination="{dest}" AssertionConsumerServiceURL="{acs}">
            <saml:Issuer xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion">{issuer}</saml:Issuer>
        </samlp:AuthnRequest>"#,
        id = Uuid::new_v4().simple(),
        ts = chrono::Utc::now().to_rfc3339(),
        dest = opts.idp_sso_url,
        acs = opts.acs_url,
        issuer = opts.sp_entity_id,
    );

    // SAML HTTP-Redirect binding: Deflate → Base64 → URL-encode
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(xml.as_bytes()).map_err(|_| AppError::Crypto)?;
    let deflated = encoder.finish().map_err(|_| AppError::Crypto)?;
    let encoded = urlencoding::encode(&B64.encode(&deflated)).into_owned();
    Ok(format!("{}?SAMLRequest={}", opts.idp_sso_url, encoded))
}

/// Parses a base64-encoded SAML response, extracts NameID, and JIT-provisions the user.
pub async fn handle_acs_response(
    pool: &PgPool,
    connection: &Connection,
    saml_response_b64: &str,
    master_key: &[u8; 32],
) -> Result<User, AppError> {
    let opts: SamlConnectionOptions = {
        let dec = crypto::decrypt(master_key, &connection.options_encrypted)?;
        serde_json::from_slice(&dec).map_err(|_| AppError::InvalidConnection)?
    };

    // SECURITY: XML-DSIG signature validation against opts.idp_certificate_pem is
    // mandatory before trusting the NameID. Integrate an XML-DSIG library before
    // enabling this code path in production. Returning an error until then.
    let _ = &opts;
    return Err(AppError::InvalidConnection);

    #[allow(unreachable_code)]
    {
        let xml_bytes = base64::engine::general_purpose::STANDARD
            .decode(saml_response_b64)
            .map_err(|_| AppError::InvalidToken)?;
        let xml = String::from_utf8(xml_bytes).map_err(|_| AppError::InvalidToken)?;
        let name_id = extract_name_id(&xml).ok_or(AppError::InvalidToken)?;
        let email = extract_attribute(&xml, "email").unwrap_or_else(|| name_id.clone());
        jit_provision_saml(pool, connection, &name_id, &email).await
    }
}

fn extract_name_id(xml: &str) -> Option<String> {
    let start = xml.find("<saml:NameID")?;
    let inner = &xml[start..];
    let content_start = inner.find('>')? + 1;
    let content_end = inner.find("</saml:NameID>")?;
    Some(inner[content_start..content_end].trim().to_owned())
}

fn extract_attribute(xml: &str, attr_name: &str) -> Option<String> {
    let needle = format!("Name=\"{}\"", attr_name);
    let start = xml.find(&needle)?;
    let after = &xml[start..];
    let val_start = after.find("<saml:AttributeValue")?;
    let val_inner = &after[val_start..];
    let content_start = val_inner.find('>')? + 1;
    let content_end = val_inner.find("</saml:AttributeValue>")?;
    Some(val_inner[content_start..content_end].trim().to_owned())
}

async fn jit_provision_saml(
    pool: &PgPool,
    connection: &Connection,
    name_id: &str,
    email: &str,
) -> Result<User, AppError> {
    if let Some(u) = sqlx::query_as!(
        User,
        r#"SELECT id, tenant_id, organization_id, connection_id, email,
                  password_hash, external_provider_id,
                  user_metadata as "user_metadata!: sqlx::types::Json<serde_json::Value>",
                  app_metadata  as "app_metadata!: sqlx::types::Json<serde_json::Value>",
                  created_at, updated_at
           FROM users
           WHERE tenant_id = $1 AND connection_id = $2 AND external_provider_id = $3"#,
        connection.tenant_id, connection.id, name_id
    )
    .fetch_optional(pool)
    .await? {
        return Ok(u);
    }

    let user = sqlx::query_as!(
        User,
        r#"INSERT INTO users (tenant_id, connection_id, email, external_provider_id)
           VALUES ($1, $2, $3, $4)
           RETURNING id, tenant_id, organization_id, connection_id, email,
                     password_hash, external_provider_id,
                     user_metadata as "user_metadata!: sqlx::types::Json<serde_json::Value>",
                     app_metadata  as "app_metadata!: sqlx::types::Json<serde_json::Value>",
                     created_at, updated_at"#,
        connection.tenant_id, connection.id, email, name_id
    )
    .fetch_one(pool)
    .await?;
    Ok(user)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_authn_request_produces_redirect_url() {
        let key = [0x42u8; 32];
        let opts = SamlConnectionOptions {
            idp_sso_url: "https://idp.example.com/sso".into(),
            idp_entity_id: "https://idp.example.com".into(),
            idp_certificate_pem: "MIICERT".into(),
            sp_entity_id: "https://idass.example.com".into(),
            acs_url: "https://idass.example.com/acs".into(),
        };
        let enc = shared_kernel::crypto::encrypt(&key, &serde_json::to_vec(&opts).unwrap()).unwrap();
        let conn = Connection {
            id: Uuid::new_v4(), tenant_id: Uuid::new_v4(),
            name: "saml".into(), strategy: "saml".into(),
            options_encrypted: enc, webhook_url: None,
            created_at: chrono::Utc::now(),
        };
        let url = generate_authn_request(&conn, &key).unwrap();
        assert!(url.starts_with("https://idp.example.com/sso?SAMLRequest="));
    }

    #[test]
    fn extract_name_id_parses_xml() {
        let xml = r#"<samlp:Response><saml:Assertion><saml:Subject>
            <saml:NameID Format="urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress">
                alice@corp.com
            </saml:NameID></saml:Subject></saml:Assertion></samlp:Response>"#;
        assert_eq!(extract_name_id(xml).as_deref(), Some("alice@corp.com"));
    }
}
