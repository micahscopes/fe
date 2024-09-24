use zed_extension_api as zed;

// Define the FeExtension struct
struct FeExtension;

// Implement the zed::Extension trait for FeExtension
impl zed::Extension for FeExtension {
    // Implement the required `new` method
    fn new() -> Self {
        FeExtension
    }

    // Correct the return type to match the trait's expectation
    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command, String> {
        // Implement the language server command here
        // For now, we'll return a placeholder command
        Ok(zed::Command {
            command: "fe-analyzer".to_string(),
            args: vec![], // Add necessary arguments if any
            env: vec![],  // Changed from `None` to an empty vector
        })
    }
}

// Register the FeExtension with Zed
zed::register_extension!(FeExtension);
