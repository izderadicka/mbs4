use std::{clone, collections::HashSet, fmt::Display, hash::Hash, str::FromStr, time::SystemTime};

use serde::{Deserialize, Serialize};

use crate::error::Error;

#[derive(Debug, Hash, PartialEq, Eq, Serialize, Deserialize, Clone, Copy)]
pub enum Role {
    Admin,
    Trusted,
    #[cfg(test)]
    JustForTest,
}

impl Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::Admin => write!(f, "admin"),
            Role::Trusted => write!(f, "trusted"),
            #[cfg(test)]
            Role::JustForTest => write!(f, "just_for_test"),
        }
    }
}

impl FromStr for Role {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "admin" => Ok(Role::Admin),
            "trusted" => Ok(Role::Trusted),
            #[cfg(test)]
            "just_for_test" => Ok(Role::JustForTest),
            _ => Err(Error::msg(format!("Invalid role: {}", s))),
        }
    }
}

pub trait TimeLimited {
    fn set_validity(&mut self, until: SystemTime);
    fn check_validity(&self) -> bool;
}

pub trait Authorization {
    fn has_role(&self, role: Role) -> bool;

    fn has_any_role<I>(&self, roles: I) -> bool
    where
        I: IntoIterator<Item = Role>,
    {
        roles.into_iter().any(|role| self.has_role(role))
    }

    fn has_any_role_ref<'a, I>(&self, roles: I) -> bool
    where
        I: IntoIterator<Item = &'a Role>,
    {
        roles.into_iter().any(|role| self.has_role(*role))
    }

    fn has_all_roles<I>(&self, roles: I) -> bool
    where
        I: IntoIterator<Item = Role>,
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
    fn has_role(&self, role: Role) -> bool {
        self.roles.contains(&role)
    }
}

#[derive(Debug, Serialize, Deserialize, clone::Clone)]
pub struct ApiClaim {
    pub sub: String,
    pub iat: u64,
    pub exp: u64,
    pub roles: HashSet<Role>,
}

impl ApiClaim {
    pub fn new_expired<R>(sub: impl Into<String>, roles: impl IntoIterator<Item = R>) -> Self
    where
        R: Into<Role>,
    {
        Self {
            sub: sub.into(),
            iat: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            exp: 0,
            roles: roles.into_iter().map(Into::into).collect(),
        }
    }
}

impl Authorization for ApiClaim {
    fn has_role(&self, role: Role) -> bool {
        self.roles.contains(&role)
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
        let role_admin = Role::Admin;
        let role_trusted = Role::Trusted;
        let role_test = Role::JustForTest;
        assert_eq!(role_admin.to_string(), "admin");
        assert_eq!(role_trusted.to_string(), "trusted");
        let claim = ApiClaim {
            sub: "123".to_string(),
            iat: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            exp: 1,
            roles: HashSet::from([role_admin, Role::Trusted]),
        };
        assert!(claim.has_role(role_admin));
        assert!(claim.has_role(role_trusted));
        assert!(!claim.has_role(role_test));
        assert!(claim.has_any_role([Role::Admin, Role::JustForTest]));
        assert!(claim.has_any_role(vec![Role::Trusted, Role::JustForTest]));
        assert!(claim.has_all_roles([Role::Admin, Role::Trusted]));
    }
}
