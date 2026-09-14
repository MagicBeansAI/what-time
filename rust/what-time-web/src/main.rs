use tiny_http::{Header, Method, Response, Server};

const INDEX_HTML: &str = include_str!("../assets/index.html");

fn respond(request: tiny_http::Request, status: u16, content_type: &str, body: Vec<u8>) {
    let header = Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()).unwrap();
    let response = Response::from_data(body)
        .with_status_code(status)
        .with_header(header);
    let _ = request.respond(response);
}

fn json_response(request: tiny_http::Request, status: u16, body: String) {
    respond(request, status, "application/json", body.into_bytes());
}

fn handle_parse(mut request: tiny_http::Request) {
    let mut body = String::new();
    if request.as_reader().read_to_string(&mut body).is_err() {
        json_response(
            request,
            400,
            r#"{"error":"unreadable request body"}"#.into(),
        );
        return;
    }
    let payload: serde_json::Value = match serde_json::from_str(&body) {
        Ok(payload) => payload,
        Err(error) => {
            json_response(
                request,
                400,
                serde_json::json!({"error": format!("invalid JSON: {error}")}).to_string(),
            );
            return;
        }
    };

    let text = payload
        .get("text")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let default_reference = default_reference();
    let reference = payload
        .pointer("/context/reference")
        .and_then(|value| value.as_str())
        .unwrap_or(default_reference.as_str());
    let time_zone = payload
        .pointer("/context/timeZone")
        .and_then(|value| value.as_str())
        .unwrap_or("UTC");
    let limit = payload
        .pointer("/context/limit")
        .and_then(|value| value.as_i64())
        .unwrap_or(30);
    let date_order = match payload.get("dateOrder").and_then(|value| value.as_str()) {
        Some("DMY") => what_time::DateOrder::DMY,
        _ => what_time::DateOrder::MDY,
    };

    let context = what_time::ParseContext {
        reference: reference.to_string(),
        time_zone: time_zone.to_string(),
        limit: Some(limit.clamp(1, 1000)),
        ..Default::default()
    };

    let parser = what_time::Parser::new(what_time::ParserOptions {
        date_order: Some(date_order),
        ..Default::default()
    });
    let result = match parser.parse(text, &context) {
        Ok(result) => result,
        Err(error) => {
            json_response(
                request,
                400,
                serde_json::json!({"error": error.to_string()}).to_string(),
            );
            return;
        }
    };
    // The intermediate schedules come from the model-level parser; inference
    // is a fraction of a millisecond, so running it twice keeps this simple.
    let expressions = match what_time::ScheduleParser::new(what_time::ParserOptions {
        date_order: Some(date_order),
        ..Default::default()
    })
    .parse(text)
    {
        Ok(parsed) => parsed.expressions,
        Err(_) => Vec::new(),
    };

    json_response(
        request,
        200,
        serde_json::json!({
            "result": serde_json::to_value(&result).unwrap(),
            "expressions": serde_json::to_value(&expressions).unwrap(),
        })
        .to_string(),
    );
}

fn default_reference() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is after the epoch")
        .as_millis() as i64;
    let seconds = now.div_euclid(1000);
    let subsecond = now.rem_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let time = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{subsecond:03}Z",
        time / 3600,
        time / 60 % 60,
        time % 60
    )
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn main() {
    let mut port: u16 = 8787;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" | "-p" => {
                port = args
                    .next()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(8787);
            }
            other => {
                if let Ok(value) = other.parse::<u16>() {
                    port = value;
                }
            }
        }
    }

    let address = format!("127.0.0.1:{port}");
    let server = Server::http(&address).unwrap_or_else(|error| {
        eprintln!("cannot bind {address}: {error}");
        std::process::exit(1);
    });
    println!("what-time test UI: http://{address}");

    for request in server.incoming_requests() {
        let path = request.url().split('?').next().unwrap_or("/").to_string();
        match (request.method(), path.as_str()) {
            (Method::Get, "/") => respond(
                request,
                200,
                "text/html; charset=utf-8",
                INDEX_HTML.as_bytes().to_vec(),
            ),
            (Method::Get, "/api/defaults") => json_response(
                request,
                200,
                serde_json::json!({"reference": default_reference()}).to_string(),
            ),
            (Method::Post, "/api/parse") => handle_parse(request),
            (Method::Get, "/health") => json_response(request, 200, "{\"ok\":true}".into()),
            _ => respond(
                request,
                404,
                "text/plain; charset=utf-8",
                b"not found".to_vec(),
            ),
        }
    }
}
