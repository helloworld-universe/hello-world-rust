mod app;
mod logging;

use hello_world::app::ExitCode;
use human_panic::{Metadata, setup_panic};
use std::time::Instant;

#[tokio::main]
async fn main() {
    //----------------------------------------------------------------------
    // 1. Install developer-friendly color-eyre (pretty backtraces)
    //----------------------------------------------------------------------
    if let Err(e) = color_eyre::install() {
        eprintln!("Failed to install color-eyre: {e}");
        std::process::exit(ExitCode::InitFailure as i32);
    }

    //----------------------------------------------------------------------
    // 2. Install human-panic last (handles panics)
    //----------------------------------------------------------------------
    setup_panic!(
        Metadata::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
            .authors("My Company Support <support@mycompany.com>")
            .homepage("support.mycompany.com")
            .support("- Open a support request by email to support@mycompany.com")
    );

    // Measure duration
    let start = Instant::now();

    //----------------------------------------------------------------------
    // 3. Set up logging
    //----------------------------------------------------------------------
    if let Err(e) = logging::init_logging() {
        eprintln!("Failed to initialize logging: {e}");
        std::process::exit(ExitCode::InitFailure as i32);
    }

    tracing::info!("Hello World App starting…");

    //----------------------------------------------------------------------
    // 4. Application main logic using rootcause::Result
    //----------------------------------------------------------------------
    match app::run().await {
        Ok(_) => {
            let duration = start.elapsed();
            let nanoseconds = duration.as_nanos(); // u128
            tracing::debug!("Elapsed time in nanoseconds: {}", nanoseconds);

            tracing::info!("Application completed successfully");
            std::process::exit(ExitCode::Ok as i32);
        }
        Err(err) => {
            let duration = start.elapsed();
            let nanoseconds = duration.as_nanos(); // u128
            tracing::debug!("Elapsed time in nanoseconds: {}", nanoseconds);

            // Log it with tracing before panicking
            tracing::error!(error = ?err, "Application error occurred");

            // Converting into a panic triggers human-panic crash reports
            panic!("Application error: {err:?}");
        }
    }
}
