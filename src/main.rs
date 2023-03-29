mod push_config_injecter;
use actix_web::{get, http, post, web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use anyhow::Result;
use kube::core::{
    admission::{AdmissionRequest, AdmissionResponse, AdmissionReview},
    DynamicObject, ResourceExt, Status,
};
use log::{error, info, warn};
use push_config_injecter::{
    get_endpoint_secret, get_inject_annotation, get_json_patch, get_server_certificate, Annotation, Secret,
};
use rustls::ServerConfig;
use serde_json::json;

fn mutation_denied(admission_res: &mut AdmissionResponse, msg: String) -> AdmissionReview<DynamicObject> {
    warn!("{}", msg);
    admission_res.allowed = false;
    admission_res.result = Status {
        message: msg,
        ..Default::default()
    };
    return admission_res.clone().into_review();
}

fn validate_content_header(request: &HttpRequest) -> Option<String> {
    if let Some(content_type) = request.head().headers.get("content-type") {
        if content_type != "application/json" {
            let msg = format!("invalid content-type: {:?}", content_type);
            info!("Warn: {}, Code: {}", msg, http::StatusCode::BAD_REQUEST);
            return Some(msg);
        }
    }
    None
}

#[get("/healthz")]
async fn health() -> impl Responder {
    HttpResponse::Ok()
        .append_header((http::header::CONTENT_TYPE, "application/json"))
        .json(json!({"message": "ok"}))
}

#[post("/mutate")]
async fn handle_mutate(
    reqst: HttpRequest,
    body: web::Json<AdmissionReview<DynamicObject>>,
) -> impl Responder {
    info!(
        "request recieved: method={:?}, uri={}",
        reqst.method(),
        reqst.uri(),
    );

    if let Some(msg) = validate_content_header(&reqst) {
        return HttpResponse::BadRequest().json(msg);
    }

    let a_req: AdmissionRequest<_> = match body.into_inner().try_into() {
        Ok(x) => x,
        Err(err) => {
            error!("invalid request: {}", err.to_string());
            return HttpResponse::InternalServerError()
                .json(&AdmissionResponse::invalid(err.to_string()).into_review());
        }
    };

    let mut a_resp = AdmissionResponse::from(&a_req);

    let req_object = match a_req.object {
        Some(x) => x,
        None => {
            return HttpResponse::InternalServerError().json("could not get object from the request body")
        }
    };

    let resource_name = req_object.name_any();

    let annotation_values = match get_inject_annotation(&req_object).await {
        Annotation::Result(x) => x,
        Annotation::NotFound => return HttpResponse::Ok().json(a_resp.into_review()),
        Annotation::Invalid => {
            let msg = format!(
                "[{}] invalid value of 'pubsub-push-config/inject-from' annotation.",
                resource_name
            );
            let admission_response = mutation_denied(&mut a_resp, msg);
            return HttpResponse::Ok().json(admission_response);
        }
    };

    let endpoint_secret = match get_endpoint_secret(&annotation_values).await.unwrap() {
        Secret::Result(x) => x,
        Secret::SecretNotFound(x) => {
            let msg = format!(
                "[{}] secret \"{}\" not found in namespace \"{}\"",
                resource_name, x.name, x.namespace
            );
            let admission_response = mutation_denied(&mut a_resp, msg);
            return HttpResponse::Ok().json(admission_response);
        }
        Secret::KeyNotFound(x) => {
            let msg = format!(
                "[{}] key \"{}\" not found in secret \"{}\"",
                resource_name, x.key, x.name
            );
            let admission_response = mutation_denied(&mut a_resp, msg);
            return HttpResponse::Ok().json(admission_response);
        }
    };

    let patches = get_json_patch(endpoint_secret).await.unwrap();

    match a_resp.with_patch(json_patch::Patch(patches)) {
        Ok(x) => return HttpResponse::Ok().json(x.into_review()),
        Err(err) => {
            let msg = format!("internal server error: {}", err.to_string());
            error!("{}", msg);
            return HttpResponse::InternalServerError().json(msg);
        }
    };
}

#[actix_web::main]
async fn main() -> Result<(), anyhow::Error> {
    env_logger::init();

    let sc = get_server_certificate()?;

    let server_config = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(sc.cert_chain, sc.private_key)?;

    info!("Started http server: 0.0.0.0:8443");

    HttpServer::new(|| App::new().service(health).service(handle_mutate))
        .bind_rustls("0.0.0.0:8443", server_config)?
        .run()
        .await?;
    Ok(())
}
