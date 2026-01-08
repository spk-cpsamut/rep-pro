use super::clilent::Client;
use aws_config;
use aws_sdk_kms::{
    self,
    config::http::HttpResponse,
    error::SdkError,
    operation::encrypt::{EncryptError, EncryptOutput},
    primitives::Blob,
};
use base64::{Engine, engine};
use std::env;
pub async fn encrypt(client: Client) -> Result<(), Box<dyn std::error::Error>> {
    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .profile_name(&env::var("AWS_PROFILE_NAME")?)
        .load()
        .await;
    let kms_client = aws_sdk_kms::Client::new(&config);


    let blob_username = Blob::new(client.username);
    let blob_password = Blob::new(client.password);
    let resoruce_config_json = serde_json::to_string(&client.config);

    let enctyped_username_future = kms_client
        .encrypt()
        .key_id(&env::var("KMS_ID")?)
        .plaintext(blob_username)
        .send();
    let enctypted_password_future = kms_client
        .encrypt()
        .key_id(&env::var("KMS_ID")?)
        .plaintext(blob_password)
        .send();

    let (enctyped_username, enctypted_password) = tokio::join!(enctyped_username_future, enctypted_password_future);

    let base64_encrypted_username =
        engine::general_purpose::STANDARD.encode(unwrap_ciphertext(enctyped_username)?);
    let base64_encrypted_password =
        engine::general_purpose::STANDARD.encode(unwrap_ciphertext(enctypted_password)?);

    println!("username : {}", base64_encrypted_username);
    println!("password : {}", base64_encrypted_password);
    println!("config : {}", resoruce_config_json?);

    Ok(())
}

fn unwrap_ciphertext(
    encypted: Result<EncryptOutput, SdkError<EncryptError, HttpResponse>>,
) -> Result<Blob, String> {
    match encypted {
        Ok(output) => match output.ciphertext_blob {
            Some(b) => Ok(b),
            None => Err("error".to_string()),
        },
        Err(_) => Err("error".to_string()),
    }
}
