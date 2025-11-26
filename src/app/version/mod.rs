use semver::Version;

pub fn is_latest_version() -> bool {
    let latest = Version::parse(get_latest_version)?;
    let current = Version::parse(get_current_version)?;
    let needs_update = current < latest;

    needs_update
}

fn get_latest_version() {
    std::env::env!("CARGO_PKG_VERSION")
}

fn get_current_version() {
    std::env::env!("CARGO_PKG_VERSION")
}
