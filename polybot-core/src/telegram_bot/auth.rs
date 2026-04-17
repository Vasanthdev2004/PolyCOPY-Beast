use std::collections::HashSet;

/// v2.5: Telegram authentication — whitelist checked on every message.
pub struct AuthService {
    allowed_user_ids: HashSet<u64>,
}

impl AuthService {
    pub fn new(user_ids: Vec<u64>) -> Self {
        Self {
            allowed_user_ids: user_ids.into_iter().collect(),
        }
    }

    pub fn is_allowed(&self, user_id: u64) -> bool {
        if self.allowed_user_ids.is_empty() {
            return false;
        }
        self.allowed_user_ids.contains(&user_id)
    }

    pub fn allowed_count(&self) -> usize {
        self.allowed_user_ids.len()
    }

    pub fn allowed_users(&self) -> Vec<u64> {
        self.allowed_user_ids.iter().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_user_passes() {
        let auth = AuthService::new(vec![123, 456]);
        assert!(auth.is_allowed(123));
        assert!(auth.is_allowed(456));
    }

    #[test]
    fn unknown_user_blocked() {
        let auth = AuthService::new(vec![123]);
        assert!(!auth.is_allowed(999));
    }

    #[test]
    fn empty_whitelist_blocks_all() {
        let auth = AuthService::new(vec![]);
        assert!(!auth.is_allowed(123));
    }
}
