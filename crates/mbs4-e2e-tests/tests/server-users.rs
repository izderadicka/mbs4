use garde::Validate as _;
use mbs4_dal::user;
use mbs4_e2e_tests::{TestUser, launch_env, prepare_env};
use mbs4_types::general::ValidEmail;
use tracing::info;
use tracing_test::traced_test;

#[tokio::test]
#[traced_test]
async fn test_invalid_user_email() {
    let (args, _config_guard) = prepare_env("test_user_invalid_email").await.unwrap();
    let user_email = "invalid";
    let user_password = "password";

    let new_user = user::CreateUser {
        name: Some("admin".to_string()),
        email: ValidEmail::cheat(user_email.to_string()),
        password: Some(user_password.to_string()),
        roles: Some(vec!["admin".to_string()]),
    };

    assert!(new_user.email.validate().is_err());
    let base_url = args.base_url.clone();

    let (client, _) = launch_env(args, TestUser::Admin).await.unwrap();

    let url = base_url.join("users").unwrap();
    info! {"Users URL: {:#?}", url};
    let response = client.post(url).json(&new_user).send().await.unwrap();
    info!("Response: {:#?}", response);
    assert!(!response.status().is_success());
    assert_eq!(422, response.status().as_u16());
    info!("Response body: {:#?}", response.text().await.unwrap());
}

#[tokio::test]
#[traced_test]
async fn test_valid_user() {
    let (args, _config_guard) = prepare_env("test_valid_user").await.unwrap();
    let new_user = user::CreateUser {
        name: Some("admin".to_string()),
        email: "admin@example.com".parse().unwrap(),
        password: Some("password".to_string()),
        roles: Some(vec!["admin".to_string(), "trusted".to_string()]),
    };

    let base_url = args.base_url.clone();

    let (client, _) = launch_env(args, TestUser::Admin).await.unwrap();

    let url = base_url.join("users").unwrap();
    info! {"Users URL: {:#?}", url};
    let response = client.post(url).json(&new_user).send().await.unwrap();
    info!("Response: {:#?}", response);
    assert!(response.status().is_success());
    assert_eq!(201, response.status().as_u16());
    info!("Response body: {:#?}", response.text().await.unwrap());
}

#[tokio::test]
#[traced_test]
async fn test_invalid_role() {
    let (args, _config_guard) = prepare_env("test_invalid_role").await.unwrap();
    let new_user = user::CreateUser {
        name: Some("admin".to_string()),
        email: "admin@example.com".parse().unwrap(),
        password: Some("password".to_string()),
        roles: Some(vec![
            "admin".to_string(),
            "trusted".to_string(),
            "usak".to_string(),
        ]),
    };

    let base_url = args.base_url.clone();

    let (client, _) = launch_env(args, TestUser::Admin).await.unwrap();

    let url = base_url.join("users").unwrap();
    info! {"Users URL: {:#?}", url};
    let response = client.post(url).json(&new_user).send().await.unwrap();
    info!("Response: {:#?}", response);
    assert!(!response.status().is_success());
    assert_eq!(422, response.status().as_u16());
    info!("Response body: {:#?}", response.text().await.unwrap());
}
