use std::{borrow::Borrow, collections::HashSet, hash::Hash, time::SystemTime};

use serde::{Deserialize, Serialize};

#[derive(Debug, Hash, PartialEq, Eq, Serialize, Deserialize, Clone)]
pub struct Role(String);

impl AsRef<str> for Role {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl Borrow<str> for Role {
    fn borrow(&self) -> &str {
        self.as_ref()
    }
}

impl Borrow<String> for Role {
    fn borrow(&self) -> &String {
        &self.0
    }
}
impl From<String> for Role {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Role {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

pub trait TimeLimited {
    fn set_validity(&mut self, until: SystemTime);
    fn check_validity(&self) -> bool;
}

pub trait Authorization {
    fn has_role<Q>(&self, role: &Q) -> bool
    where
        Role: Borrow<Q>,
        Q: Hash + Eq + ?Sized;

    fn has_any_role<'a, Q, I>(&self, roles: I) -> bool
    where
        Role: Borrow<Q>,
        Q: Hash + Eq + ?Sized + 'a,
        I: IntoIterator<Item = &'a Q>,
    {
        roles.into_iter().any(|role| self.has_role(role))
    }

    fn has_all_roles<'a, Q, I>(&self, roles: I) -> bool
    where
        Role: Borrow<Q>,
        Q: Hash + Eq + ?Sized + 'a,
        I: IntoIterator<Item = &'a Q>,
    {
        roles.into_iter().all(|role| self.has_role(role))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserClaim {
    pub sub: String,
    pub username: String,
    pub email: String,
    pub roles: HashSet<Role>,
}

impl Authorization for UserClaim {
    fn has_role<Q>(&self, role: &Q) -> bool
    where
        Role: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.roles.contains(role)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiClaim {
    pub sub: String,
    pub exp: u64,
    pub roles: HashSet<Role>,
}

impl Authorization for ApiClaim {
    fn has_role<Q>(&self, role: &Q) -> bool
    where
        Role: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.roles.contains(role)
    }
}

impl TimeLimited for ApiClaim {
    fn set_validity(&mut self, until: SystemTime) {
        self.exp = until
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }

    fn check_validity(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.exp > now
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role() {
        let role = Role("admin".to_string());
        assert_eq!(role.as_ref(), "admin");
        let claim = ApiClaim {
            sub: "123".to_string(),
            exp: 1,
            roles: HashSet::from([role.clone(), "guest".into()]),
        };
        assert!(claim.has_role(&role));
        assert!(claim.has_role("admin"));
        assert!(claim.has_role(&"admin".to_string()));
        assert!(!claim.has_role("user"));
        assert!(claim.has_any_role(["admin", "user"]));
        assert!(claim.has_any_role(vec!["admin", "user"]));
        assert!(claim.has_all_roles(["admin", "guest"]));
    }
}
