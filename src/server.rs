use std::{
    collections::HashMap,
    error::Error,
    sync::Arc,
};
use std::ops::Deref;
use std::sync::Mutex;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    Router,
    routing::get,
};
use clap::Parser;
use pflow_metamodel::compression::unzip_encoded;
use pflow_metamodel::oid;
use pflow_metamodel::petri_net::PetriNet;
use tower_http::trace::TraceLayer;

use crate::storage::{Storage, Zblob};


async fn src_handler(
    Path(ipfs_cid): Path<String>,
    state: State<Arc<Mutex<Storage>>>,
) -> impl IntoResponse {
    let zblob = state.lock().unwrap()
        .get_by_cid("pflow_models", &*ipfs_cid)
        .unwrap_or(Option::from(Zblob::default()))
        .unwrap_or(Zblob::default());

    let encoded_str = zblob.base64_zipped;
    let data = unzip_encoded(&*encoded_str, "model.json").unwrap_or("".to_string());
    let content_type = "application/json charset=utf-8";
    let status = StatusCode::OK;
    Response::builder()
        .status(status)
        .header("Content-Type", content_type)
        .body(data)
        .unwrap()
}

async fn img_handler(
    Path(ipfs_cid): Path<String>,
    state: State<Arc<Mutex<Storage>>>,
) -> impl IntoResponse {
    let zblob = state.lock().unwrap()
        .get_by_cid("pflow_models", &*ipfs_cid)
        .unwrap_or(Option::from(Zblob::default()))
        .unwrap_or(Zblob::default());

    let data = unzip_encoded(&zblob.base64_zipped, "model.json").unwrap_or("".to_string());
    let content_type = "application/json charset=utf-8";
    let status = StatusCode::OK;
    Response::builder()
        .status(status)
        .header("Content-Type", content_type)
        .body(data)
        .unwrap()
}

fn index_response(cid: String, data: String) -> impl IntoResponse {
    let html = format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width,initial-scale=1"/>
    <title>pflow.dev | metamodel editor</title>
    <script>
        sessionStorage.cid = "{}";
        sessionStorage.data = "{}".replaceAll(' ', '+');
    </script>
    <script defer="defer" src="https://cdn.jsdelivr.net/gh/pflow-dev/pflow-js@1.1.2/p/static/js/main.5dc69f67.js"> </script>
    <link href="https://cdn.jsdelivr.net/gh/pflow-dev/pflow-js@1.1.2/p/static/css/main.63d515f3.css" rel="stylesheet">
</head>
<body>
    <noscript>You need to enable JavaScript to run this app.</noscript>
    <div id="root"></div>
</body></html>
"#, cid, data);

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html charset=utf-8")
        .body(html)
        .unwrap()
}

fn index_response_redirect(cid: String) -> impl IntoResponse {
    let uri = format!("/p/{}/", cid);
    Response::builder()
        .status(StatusCode::FOUND)
        .header("Location", uri)
        .body("".to_string())
        .unwrap()
}

async fn model_handler(
    Path(ipfs_cid): Path<String>,
    req: Query<HashMap<String, String>>,
    state: State<Arc<Mutex<Storage>>>,
) -> impl IntoResponse {
    let zparam = req.get("z");
    let zblob = string_to_zblob(zparam);

    let new_blob = state.lock().unwrap().create_or_retrieve(
        "pflow_models",
        &zblob.ipfs_cid,
        &zblob.base64_zipped,
        &zblob.title,
        &zblob.description,
        &zblob.keywords,
        &zblob.referrer,
    ).unwrap_or(zblob);

    if zparam.is_some() && new_blob.id > 0 {
        return index_response_redirect(new_blob.ipfs_cid).into_response();
    }

    let zblob = state.lock().unwrap()
        .get_by_cid("pflow_models", &*ipfs_cid)
        .unwrap_or(Option::from(Zblob::default()))
        .unwrap_or(Zblob::default());

    index_response(zblob.ipfs_cid, zblob.base64_zipped).into_response()
}

fn string_to_zblob(data: Option<&String>) -> Zblob {
    let mut zblob = Zblob::default();
    if data.is_some() {
        zblob.base64_zipped = data.unwrap().to_string();
        zblob.ipfs_cid = oid::Oid::new(data.unwrap().as_bytes()).unwrap().to_string();
    }

    zblob
}

async fn index_handler(
    req: Query<HashMap<String, String>>,
    state: State<Arc<Mutex<Storage>>>,
) -> impl IntoResponse {
    let zblob = string_to_zblob(req.get("z"));
    let new_blob = state.lock().unwrap().create_or_retrieve(
        "pflow_models",
        &zblob.ipfs_cid,
        &zblob.base64_zipped,
        &zblob.title,
        &zblob.description,
        &zblob.keywords,
        &zblob.referrer,
    ).unwrap_or(Zblob::default());

    if new_blob.id > 0 {
        let redirect_uri = format!("/p/{}/", zblob.ipfs_cid);
        return Redirect::permanent(&*redirect_uri).into_response();
    }

    return index_response(zblob.ipfs_cid, zblob.base64_zipped).into_response();
}

pub fn app() -> Router {
    let store = Storage::new("pflow.db").unwrap();
    store.create_tables().unwrap();
    let state: Arc<Mutex<Storage>> = Arc::new(Mutex::new(store));

    // Build route service
    Router::new()
        .route("/img/:ipfs_cid.svg", get(img_handler))
        .route("/src/:ipfs_cid.json", get(src_handler))
        .route("/p/:ipfs_cid/", get(model_handler))
        .route("/p/", get(get(index_handler)))
        .route("/p", get(|| async { Redirect::to("/p/") }))
        .route("/", get(|| async { Redirect::to("/p/") }))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use pflow_metamodel::compression::unzip_encoded;
    use pflow_metamodel::petri_net::PetriNet;
    use crate::fixtures::INHIBIT_TEST;
    use crate::server::string_to_zblob;
    use crate::storage::Storage;

    #[test]
    fn test_serve_by_ipfs_cid() {
        let p = string_to_zblob(Option::from(&INHIBIT_TEST.to_string()));
        let net = PetriNet::from_json(unzip_encoded(&p.base64_zipped, "model.json").unwrap()).unwrap();
        let store = Storage::new("pflow.db").unwrap();
        store.reset_db(true).unwrap();
        let z = string_to_zblob(Option::from(&INHIBIT_TEST.to_string()));
        assert_eq!(z.ipfs_cid, "zb2rhjSDP7JbLBEjfeThBdnE2va1sTahmDkooKB9VYGD67tf5");
        let store = Storage::new("pflow.db").unwrap();

        for i in 0..3 {
            match store.create_or_retrieve(
                "pflow_models",
                &z.ipfs_cid,
                &z.base64_zipped,
                &z.title,
                &z.description,
                &z.keywords,
                &z.referrer,
            ) {
                Ok(zblob) => {
                    assert_eq!(zblob.ipfs_cid, z.ipfs_cid);
                    assert_eq!(zblob.id, 1);
                }
                Err(e) => {
                    panic!("Failed to create zblob: {}", e);
                }
            }
        }
    }
}