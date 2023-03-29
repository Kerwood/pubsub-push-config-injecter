use anyhow::Result;
use json_patch::PatchOperation;
use k8s_openapi::api::core::v1;
use kube::core::{DynamicObject, ResourceExt};
use kube::{Api, Client};
use log::{debug, info};
use rcgen::{Certificate, CertificateParams, KeyPair, SanType};
use rustls::PrivateKey;
use rustls_pemfile;
use std::env;
use std::fs;
use std::io::BufReader;

#[derive(Debug, Clone)]
pub struct AnnotationValues {
    pub namespace: String,
    pub name: String,
    pub key: String,
}

pub enum Annotation {
    Result(AnnotationValues),
    NotFound,
    Invalid,
}

pub enum Secret {
    Result(String),
    SecretNotFound(AnnotationValues),
    KeyNotFound(AnnotationValues),
}

#[derive(Debug, Clone)]
pub struct ServerCertificate {
    pub cert_chain: Vec<rustls::Certificate>,
    pub private_key: PrivateKey,
}

pub async fn get_inject_annotation(admission_data: &DynamicObject) -> Annotation {
    let annotations = &admission_data.annotations();

    if !annotations.contains_key("pubsub-push-config/inject-from") {
        debug!("[{}] no annotation found, skipping...", admission_data.name_any());
        return Annotation::NotFound;
    };

    let annotation = &annotations["pubsub-push-config/inject-from"];

    let values = async {
        let mut a = annotation.split("/");
        let namespace = a.next()?.to_owned();
        let name = a.next()?.to_owned();
        let key = a.next()?.to_owned();

        debug!(
            "annotation values: namespace={}, secret_name={}, secret_key={}",
            namespace, name, key
        );

        Some(AnnotationValues { namespace, name, key })
    }
    .await;

    match values {
        Some(x) => return Annotation::Result(x),
        None => return Annotation::Invalid,
    }
}

pub async fn get_endpoint_secret(annotation_values: &AnnotationValues) -> Result<Secret> {
    let client = Client::try_default().await?;

    let namespace_secrets: Api<v1::Secret> = Api::namespaced(client, &annotation_values.namespace);

    let secret_keys = match namespace_secrets.get_opt(&annotation_values.name).await? {
        Some(secret) => secret.data,
        None => return Ok(Secret::SecretNotFound(annotation_values.clone())),
    };

    let result = match secret_keys.and_then(|x| x.get(&annotation_values.key).cloned()) {
        Some(x) => String::from_utf8(x.0.to_owned())?,
        None => return Ok(Secret::KeyNotFound(annotation_values.clone())),
    };

    Ok(Secret::Result(result))
}

pub async fn get_json_patch(endpoint_secret: String) -> Result<Vec<PatchOperation>> {
    let mut patches: Vec<PatchOperation> = vec![];

    patches.push(json_patch::PatchOperation::Add(json_patch::AddOperation {
        path: "/spec/pushConfig".into(),
        value: serde_json::json!({}),
    }));

    patches.push(json_patch::PatchOperation::Add(json_patch::AddOperation {
        path: "/spec/pushConfig/pushEndpoint".into(),
        value: serde_json::to_value(endpoint_secret)?,
    }));
    return Ok(patches);
}

pub fn get_server_certificate() -> Result<ServerCertificate> {
    let namespace = match env::var_os("NAMESPACE") {
        Some(x) => {
            let x = x
                .into_string()
                .expect("Couldn't convert namespace OsString to String");
            info!(
                "NAMESPACE environment variable found. Using '{}' for certificate SAN configuration.",
                x
            );
            x
        }
        None => {
            info!(
                "No NAMESPACE environment variable found, using 'default' for certificate SAN configuration."
            );
            "default".to_string()
        }
    };

    let ca_key_pem = match fs::read_to_string("./certs/ca.key") {
        Ok(x) => x,
        Err(_) => fs::read_to_string("./certs/tls.key").expect("Couldn't read CA key file"),
    };

    let ca_crt_pem = match fs::read_to_string("./certs/ca.crt") {
        Ok(x) => x,
        Err(_) => fs::read_to_string("./certs/tls.crt").expect("Couldn't read CA certificate file"),
    };

    let ca_key = KeyPair::from_pem(&ca_key_pem).expect("Cannot create KeyPair from pem_str");

    let mut params =
        CertificateParams::from_ca_cert_pem(&ca_crt_pem, ca_key).expect("Couldn't create CertificateParams");

    params.subject_alt_names = vec![
        SanType::DnsName("push-config-injecter".to_string()),
        SanType::DnsName(format!("push-config-injecter.{}", namespace)),
        SanType::DnsName(format!("push-config-injecter.{}.svc", namespace)),
        SanType::DnsName(format!("push-config-injecter.{}.svc.cluster", namespace)),
        SanType::DnsName(format!("push-config-injecter.{}.svc.cluster.local", namespace)),
    ];

    let new_certificate = Certificate::from_params(params).expect("Couldn't create Certificate");
    let certificate_pem = new_certificate
        .serialize_pem()
        .expect("Couldn't serialize certificate to pem string");
    let certificate_key = new_certificate.serialize_private_key_pem();

    let certificate_buff_reader = &mut BufReader::new(certificate_pem.as_bytes());
    let certificate_key_buff_reader = &mut BufReader::new(certificate_key.as_bytes());

    let cert_chain: Vec<rustls::Certificate> = rustls_pemfile::certs(certificate_buff_reader)
        .expect("Couldn't extract certificate from BufReader")
        .into_iter()
        .map(rustls::Certificate)
        .collect();

    let keys = rustls_pemfile::pkcs8_private_keys(certificate_key_buff_reader)
        .expect("Couldn't extract PKCS8-encoded private keys")
        .pop()
        .expect("Couldn't not convert private key to DER");

    Ok(ServerCertificate {
        cert_chain,
        private_key: PrivateKey(keys),
    })
}
