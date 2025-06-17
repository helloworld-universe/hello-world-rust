use cucumber::{World, given, then, when, gherkin::Step};
use reqwest::{Client, StatusCode};
use serde_json::Value;
use hello_world::app;

// --- 1. Define the Test World State ---

// The state passed between steps
#[derive(Debug, World)]
pub struct ApiWorld {
    client: Client,
    server_url: String,
    response_status: Option<StatusCode>,
    response_body: Option<Value>,
}

impl Default for ApiWorld {
    fn default() -> Self {
        ApiWorld {
            client: Client::new(),
            server_url: String::new(),
            response_status: None,
            response_body: None,
        }
    }
}

// --- 2. Step Implementations ---

// Given the server is running on "http://127.0.0.1:3000"
#[given(regex = "the server is running on \"(.*)\"")]
async fn server_is_running(world: &mut ApiWorld, url: String) {
    // NOTE: In a real test, you'd start the server here on a test port
    // and wait for it to be ready. For simplicity, we just store the URL.
    world.server_url = url;
}

// When I send a GET request to "/user"
#[when(regex = "I send a (GET|POST|PUT|DELETE) request to \"(.*)\"")]
async fn send_request(world: &mut ApiWorld, method: String, path: String) {
    let url = format!("{}{}", world.server_url, path);
    let method = method.as_str();

    let res = match method {
        "GET" => world.client.get(&url).send().await,
        // Add other methods as needed
        _ => panic!("Unsupported method in test step: {}", method),
    }
    .expect("Failed to send request");

    world.response_status = Some(res.status());
    world.response_body = Some(res.json().await.expect("Failed to parse JSON body"));
}

// Then the response status code should be 200
#[then(regex = "the response status code should be (\\d+)")]
fn check_status_code(world: &mut ApiWorld, expected_code: u16) {
    let expected = StatusCode::from_u16(expected_code).unwrap();
    assert_eq!(
        world.response_status,
        Some(expected),
        "Status code mismatch"
    );
}

// And the response body should match JSON:
#[then("the response body should match JSON:")]
fn check_response_json(world: &mut ApiWorld, step: &Step) {
    let expected_json_str = step.docstring().expect("Expected JSON docstring");
    let expected_body: Value =
        serde_json::from_str(expected_json_str).expect("Invalid expected JSON");

    let actual_body = world
        .response_body
        .as_ref()
        .expect("Response body is missing");

    assert_eq!(actual_body, &expected_body, "Response JSON mismatch");
}

// --- 3. Test Runner Entry Point ---

// Define the entry point for the cucumber tests
#[tokio::main]
async fn main() {
    // 1. --- LAUNCH SERVER (BEFORE TESTS) ---
    // Start the server in a separate Tokio task.

    let server_handle: tokio::task::JoinHandle<()> = tokio::spawn(async {
        println!("🚀 Starting mock server on 127.0.0.1:3000...\n");

        match app::run().await {
            Ok(_) => {
                tracing::info!("✅ Application completed successfully");
            }
            Err(err) => {
                // Log it with tracing before panicking
                tracing::error!(error = ?err, "Application error occurred");

                // Converting into a panic triggers human-panic crash reports
                panic!("Application error: {err:?}");
            }
        }

        println!("✅ Mock server ready.");
        // We'll let this task run until the cucumber runner finishes.
        // Wait indefinitely (or on a signal channel) to keep the task alive
        // until we call `abort()` on the handle.
        std::future::pending::<()>().await;
    });

    // Give the server a moment to bind and start listening
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    // 2. --- RUN TESTS ---
    println!("🧪 Running Cucumber tests...");
    let _runner_result = ApiWorld::run("features/").await;
    println!("🏁 Tests finished.");

    // 3. --- SHUTDOWN SERVER (AFTER TESTS) ---
    server_handle.abort(); // Forcefully stop the background server task
    println!("🛑 Server shut down.");

    // Check test results and exit appropriately (common for CI/CD)
    //if runner_result.is_err() {
    //    std::process::exit(1);
    //}
}
