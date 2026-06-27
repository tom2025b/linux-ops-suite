#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedCommand {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
}

pub(crate) fn parse_command(command: &str) -> Result<ParsedCommand, String> {
    if command.trim().is_empty() {
        return Err("Command is empty".to_string());
    }

    let words = shlex::split(command).ok_or_else(|| {
        format!(
            "Could not parse command {:?}: unmatched shell quote",
            command
        )
    })?;

    let mut words = words.into_iter();
    let program = words
        .next()
        .ok_or_else(|| "Command is empty after parsing".to_string())?;

    Ok(ParsedCommand {
        program,
        args: words.collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_arguments() {
        let parsed = parse_command("cargo test -- 'one test'").unwrap();

        assert_eq!(parsed.program, "cargo");
        assert_eq!(parsed.args, vec!["test", "--", "one test"]);
    }

    #[test]
    fn rejects_empty_commands() {
        assert_eq!(parse_command("   ").unwrap_err(), "Command is empty");
    }

    #[test]
    fn reports_unmatched_quotes() {
        let error = parse_command("echo 'unterminated").unwrap_err();

        assert!(error.contains("unmatched shell quote"));
    }
}
