//! `workspaceai-mcp <memory|artifacts|vault>`: the same stdio servers the app serves as `workspaceai mcp <kind>`.
fn main() {
    let kind = std::env::args().nth(1).unwrap_or_default();
    std::process::exit(workspaceai_supervisor::mcp::run(&kind));
}
