use axum::http::HeaderMap;
use std::collections::HashMap;

/// Check whether the request is authorized.
///
/// - If `token_config` is empty, all requests are allowed.
/// - Otherwise, the request must supply the token via `Authorization: Bearer <token>`
///   header or `?token=<token>` query parameter.
pub fn check_auth(token_config: &str, headers: &HeaderMap, query: &HashMap<String, String>) -> bool {
    if token_config.is_empty() {
        return true;
    }

    // Check Authorization header
    if let Some(auth) = headers.get("authorization") {
        if let Ok(val) = auth.to_str() {
            if let Some(bearer) = val.strip_prefix("Bearer ") {
                if bearer == token_config {
                    return true;
                }
            }
        }
    }

    // Check query parameter (needed for WebSocket clients that can't set headers)
    if let Some(token) = query.get("token") {
        if token == token_config {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_token_allows_all() {
        let headers = HeaderMap::new();
        let query = HashMap::new();
        assert!(check_auth("", &headers, &query));
    }

    #[test]
    fn valid_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer my-secret".parse().unwrap());
        let query = HashMap::new();
        assert!(check_auth("my-secret", &headers, &query));
    }

    #[test]
    fn invalid_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer wrong".parse().unwrap());
        let query = HashMap::new();
        assert!(!check_auth("my-secret", &headers, &query));
    }

    #[test]
    fn valid_query_param_token() {
        let headers = HeaderMap::new();
        let mut query = HashMap::new();
        query.insert("token".into(), "my-secret".into());
        assert!(check_auth("my-secret", &headers, &query));
    }

    #[test]
    fn invalid_query_param_token() {
        let headers = HeaderMap::new();
        let mut query = HashMap::new();
        query.insert("token".into(), "wrong".into());
        assert!(!check_auth("my-secret", &headers, &query));
    }

    #[test]
    fn no_credentials_rejected() {
        let headers = HeaderMap::new();
        let query = HashMap::new();
        assert!(!check_auth("my-secret", &headers, &query));
    }
}
