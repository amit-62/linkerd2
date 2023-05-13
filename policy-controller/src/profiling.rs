use pprof::{protos::Message, ProfilerGuardBuilder};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use warp::{http::Response, Filter};

const PPROF_PORT: u16 = 81;

#[derive(Deserialize, Serialize)]
struct QueryParams {
    format: String,
}

pub fn init() {
    let guard = Arc::new(
        ProfilerGuardBuilder::default()
            .frequency(1000)
            .blocklist(&["libc", "libgcc", "pthread", "vdso"])
            .build()
            .unwrap(),
    );

    let opt_query = warp::query::<QueryParams>()
        .map(Some)
        .or_else(|_| async { Ok::<(Option<QueryParams>,), std::convert::Infallible>((None,)) });

    let pprof_report_endpoint =
        warp::path("pprof")
            .and(opt_query)
            .map(move |params: Option<QueryParams>| {
                let report = guard.report().build().unwrap();

                match params {
                    Some(obj) => {
                        if obj.format == "flamegraph" {
                            let mut file = Vec::new();
                            report.flamegraph(&mut file).unwrap();
                            Response::builder()
                                .header("content-type", "image/svg+xml")
                                .body(file)
                                .unwrap()
                        } else if obj.format == "proto" {
                            let mut file = Vec::new();
                            let profile = report.pprof().unwrap();
                            profile.write_to_vec(&mut file).unwrap();
                            Response::builder()
                                .header("Content-Type", "application/octet-stream")
                                .header("Content-Disposition", "attachment; filename=profile.pb")
                                .body(file)
                                .unwrap()
                        } else {
                            Response::builder()
                                .body(Vec::from("unknown value for format"))
                                .unwrap()
                        }
                    }
                    None => Response::builder()
                        .body(Vec::from("Failed to decode query param."))
                        .unwrap(),
                }
            });

    tokio::spawn(async move {
        warp::serve(pprof_report_endpoint)
            .run(([0, 0, 0, 0], PPROF_PORT))
            .await;
    });
}
