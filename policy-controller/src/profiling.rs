use pprof::{protos::Message, ProfilerGuardBuilder, Report};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::fmt::Write;
use tracing_flame::FlameLayer;
use tracing_subscriber::{prelude::*, fmt, Registry};
use tracing;
use warp::{http::Response, Filter};

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

    let tracing_flame_buffer = Vec::new();
    let fmt_layer = fmt::Layer::default();
    let flame_layer = FlameLayer::new(tracing_flame_buffer.clone());
    let flush_guard = flame_layer.flush_on_drop();
    let _flush = flush_guard.flush();
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
                                .header("content-disposition", "attachment; filename=folded")
                                .body(file)
                                .unwrap()
                        } else if obj.format == "tracing-flamegraph" {
                            use std::io::Write;
                            // let flush_guard = flame_layer.flush_on_drop();
                            // let flush = f_guard.flush();
                            let mut file = Vec::new();
                            file.write_all(&tracing_flame_buffer).expect("Failed to set the default subscriber");
                            Response::builder()
                                .header("content-type", "text/plain")
                                .header("content-disposition", "attachment; filename=tfolded")
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
