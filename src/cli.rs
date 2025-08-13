/// Options parsed from command line arguments
#[derive(Debug, Clone)]
pub struct CliOptions {
    pub preserve_ansi: bool,
    pub watch_binary: bool,
    pub command: String,
    pub args: Vec<String>,
}

impl CliOptions {
    /// Parse command line arguments
    /// Format: wrap-mcp [options] -- <command> [args...]
    /// Options:
    ///   --ansi  Preserve ANSI escape sequences
    ///   -w      Watch binary file for changes
    pub fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        Self::parse(&args)
    }

    /// Parse from a given argument list
    pub fn parse(args: &[String]) -> Self {
        // Find the "--" separator
        let separator_pos = args.iter().position(|arg| arg == "--");

        // Check for options before "--"
        let (preserve_ansi, watch_binary) = separator_pos
            .map(|pos| {
                let opts = &args[1..pos];
                (
                    opts.contains(&"--ansi".to_string()),
                    opts.contains(&"-w".to_string()),
                )
            })
            .unwrap_or((false, false));

        // Extract command and arguments after "--"
        let (command, wrappee_args) = match separator_pos {
            Some(pos) if pos + 1 < args.len() => {
                let command = args[pos + 1].clone();
                let wrappee_args = args
                    .get(pos + 2..)
                    .map(|slice| slice.to_vec())
                    .unwrap_or_default();
                (command, wrappee_args)
            }
            _ => {
                tracing::warn!(
                    "No wrappee command specified. Usage: wrap-mcp [options] -- <command> [args...]"
                );
                (
                    "echo".to_string(),
                    vec!["No wrappee command specified".to_string()],
                )
            }
        };

        tracing::info!("Parsed CLI options: command={command}, args={wrappee_args:?}, preserve_ansi={preserve_ansi}, watch={watch_binary}");

        Self {
            preserve_ansi,
            watch_binary,
            command,
            args: wrappee_args,
        }
    }

    /// Check if colors should be disabled
    pub fn disable_colors(&self) -> bool {
        !self.preserve_ansi
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_with_command() {
        let args = vec![
            "wrap-mcp".to_string(),
            "--".to_string(),
            "echo".to_string(),
            "hello".to_string(),
        ];
        let opts = CliOptions::parse(&args);
        assert_eq!(opts.command, "echo");
        assert_eq!(opts.args, vec!["hello"]);
        assert!(!opts.preserve_ansi);
        assert!(!opts.watch_binary);
    }

    #[test]
    fn test_parse_with_options() {
        let args = vec![
            "wrap-mcp".to_string(),
            "--ansi".to_string(),
            "-w".to_string(),
            "--".to_string(),
            "cat".to_string(),
        ];
        let opts = CliOptions::parse(&args);
        assert_eq!(opts.command, "cat");
        assert!(opts.preserve_ansi);
        assert!(opts.watch_binary);
    }

    #[test]
    fn test_parse_no_command() {
        let args = vec!["wrap-mcp".to_string()];
        let opts = CliOptions::parse(&args);
        assert_eq!(opts.command, "echo");
        assert_eq!(opts.args, vec!["No wrappee command specified"]);
    }
}