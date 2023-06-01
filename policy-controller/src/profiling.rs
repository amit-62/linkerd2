use pprof::{protos::Message, ProfilerGuardBuilder, Report};
use serde::{Deserialize, Serialize};
use std::fmt::Write;
use std::sync::Arc;
use tracing_flame::FlameLayer;
use tracing_subscriber::{fmt, prelude::*, Registry};
use warp::{http::Response, http::StatusCode, Filter};

const PPROF_PORT: u16 = 8081;

#[derive(Deserialize, Serialize)]
struct QueryParams {
    format: String,
}

struct MyReport(Report);

impl MyReport {
    pub fn folded<W>(&self, mut writer: W) -> Result<(), std::io::Error>
    where
        W: std::io::Write,
    {
        let lines: Vec<String> = self
            .0
            .data
            .iter()
            .map(|(key, value)| {
                let mut line = key.thread_name_or_id();
                line.push(';');

                for frame in key.frames.iter().rev() {
                    for symbol in frame.iter().rev() {
                        write!(&mut line, "{};", symbol).unwrap();
                    }
                }

                line.pop().unwrap_or_default();
                write!(&mut line, " {}", value).unwrap();

                line
            })
            .collect();

        for line in lines {
            writeln!(writer, "{}", line)?;
        }

        Ok(())
    }
}

pub fn init() {
    let guard = Arc::new(
        ProfilerGuardBuilder::default()
            .frequency(1000)
            .blocklist(&["libc", "libgcc", "pthread", "vdso"])
            .build()
            .unwrap(),
    );

    let (flame_layer, flame_guard) = FlameLayer::with_file("/tmp/tracing.folded").unwrap();
    let flame_guard = Arc::new(flame_guard);
    let fmt_layer = fmt::Layer::default();
    let subscriber = Registry::default().with(fmt_layer).with(flame_layer);
    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set the default subscriber");

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
                                .header("content-type", "application/octet-stream")
                                .header("content-disposition", "attachment; filename=profile.pb")
                                .body(file)
                                .unwrap()
                        } else if obj.format == "folded" {
                            let my_report = MyReport(report);
                            let mut file = Vec::new();
                            my_report.folded(&mut file).unwrap();
                            Response::builder()
                                .header("content-type", "text/plain")
                                .header(
                                    "content-disposition",
                                    "attachment; filename=profile.folded",
                                )
                                .body(file)
                                .unwrap()
                        } else if obj.format == "tracing-flamegraph" {
                            flame_guard.flush().unwrap();
                            let file = std::fs::read("/tmp/tracing.folded").unwrap();
                            Response::builder()
                                .header("content-type", "text/plain")
                                .header(
                                    "content-disposition",
                                    "attachment; filename=profile.folded",
                                )
                                .body(file)
                                .unwrap()
                        } else {
                            Response::builder()
                                .status(StatusCode::NOT_FOUND)
                                .body(Vec::from("404 Not Found"))
                                .unwrap()
                        }
                    }
                    None => Response::builder()
                        .status(StatusCode::BAD_REQUEST)
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
