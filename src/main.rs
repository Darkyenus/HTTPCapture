use std::collections::VecDeque;
use actix_web::web::Data;
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use build_html::{Container, ContainerType, Html, HtmlContainer, HtmlPage};
use std::sync::Mutex;
use actix_web::http::header::{ContentDisposition, DispositionParam, DispositionType};

async fn index(data: Data<CapturedRequests>) -> impl Responder {
    match data.requests.lock() {
        Ok(req) => {
            let page = HtmlPage::new()
                .with_title("HTTP Captures")
                .with_header(1, "HTTP Captures")
                .with_container(
                    if req.is_empty() {
                        Container::new(ContainerType::Div).with_paragraph("No captures, go to /capture to capture something")
                    } else {
                        let mut list = Container::new(ContainerType::UnorderedList);

                        for (id, capture) in &*req {
                            list.add_link(format!("/download/{}", id), format!("Download capture #{id} ({} bytes)", capture.len()))
                        }

                        list
                    }
                )
                .to_html_string();

            HttpResponse::Ok().content_type("text/html").body(page)
        }
        Err(_) => {
            HttpResponse::InternalServerError().body("Data is poisoned")
        }
    }
}

async fn download(path: web::Path<u64>, data: Data<CapturedRequests>) -> impl Responder {
    match data.requests.lock() {
        Ok(req) => {
            let id: u64 = *path.as_ref();

            let index = if let Some((first_id, _)) = req.front() {
                id.saturating_sub(*first_id)
            } else {
                0
            };

            if let Some((_, capture)) = req.get(index as usize) {
                let is_text = std::str::from_utf8(capture).is_ok();

                HttpResponse::Ok()
                    .content_type("application/octet-stream")
                    .insert_header(ContentDisposition {
                        disposition: DispositionType::Attachment,
                        parameters: vec![
                            DispositionParam::Filename(String::from(format!("capture{id}.{}", if is_text { "txt" } else { "bin" }))),
                        ],
                    })
                    .body(capture.clone())
            } else {
                HttpResponse::NotFound().body(format!("Capture #{id} not found"))
            }
        }
        Err(_) => {
            HttpResponse::InternalServerError().body("Data is poisoned")
        }
    }
}

async fn capture(req: HttpRequest, body: web::Payload, data: Data<CapturedRequests>) -> impl Responder {
    let mut capture: Vec<u8> = Vec::new();
    capture.extend_from_slice(req.head().method.as_str().as_bytes());
    capture.extend_from_slice(" ".as_bytes());
    capture.extend_from_slice(req.uri().path().as_bytes());
    capture.extend_from_slice("\r\n".as_bytes());
    for (key, value) in req.head().headers() {
        capture.extend_from_slice(key.as_str().as_bytes());
        capture.extend_from_slice(": ".as_bytes());
        capture.extend_from_slice(value.as_bytes());
        capture.extend_from_slice("\r\n".as_bytes());
    }
    capture.extend_from_slice("\r\n".as_bytes());
    match body.to_bytes().await {
        Ok(bytes) => {
            capture.extend_from_slice(bytes.as_ref());
        }
        Err(e) => {
            eprintln!("Error when capturing: {e}");
            return HttpResponse::InternalServerError().body("Failed to capture");
        }
    }

    let id = match data.requests.lock() {
        Ok(mut req) => {
            let id = if let Some((id, _)) = req.back() {
                id + 1
            } else {
                1
            };
            req.push_back((id, capture));

            if req.len() > 10 {
                req.pop_front();
            }

            id
        }
        Err(e) => {
            eprintln!("Error when capturing: {e}");
            return HttpResponse::InternalServerError().body("Failed to capture");
        }
    };

    eprintln!("Captured #{id}");

    HttpResponse::Ok().body(format!("Captured #{}", id))
}

struct CapturedRequests {
    requests: Mutex<VecDeque<(u64, Vec<u8>)>>,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let app_data = web::Data::new(CapturedRequests {
        requests: Mutex::new(VecDeque::new()),
    });

    HttpServer::new(move || {
        App::new()
            .app_data(app_data.clone())
            .route("/capture", web::route().to(capture))
            .route("/download/{id}", web::get().to(download))
            .route("/", web::get().to(index))
    })
        .bind(("0.0.0.0", 8080))?
        .run()
        .await
}