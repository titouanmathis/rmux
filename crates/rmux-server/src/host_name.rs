pub(crate) fn local_hostname() -> Option<String> {
    hostname_from_sources(
        std::env::var("HOSTNAME").ok(),
        std::env::var("COMPUTERNAME").ok(),
        std::fs::read_to_string("/etc/hostname").ok(),
    )
}

fn hostname_from_sources(
    hostname_env: Option<String>,
    computername_env: Option<String>,
    etc_hostname: Option<String>,
) -> Option<String> {
    [hostname_env, computername_env, etc_hostname]
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_owned())
        .find(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::hostname_from_sources;

    #[test]
    fn hostname_prefers_hostname_env() {
        assert_eq!(
            hostname_from_sources(
                Some(" unix-host ".to_owned()),
                Some("WIN-HOST".to_owned()),
                Some("etc-host".to_owned()),
            ),
            Some("unix-host".to_owned())
        );
    }

    #[test]
    fn hostname_uses_windows_computername_when_hostname_is_missing() {
        assert_eq!(
            hostname_from_sources(None, Some(" WIN-HOST ".to_owned()), None),
            Some("WIN-HOST".to_owned())
        );
    }

    #[test]
    fn hostname_falls_back_to_etc_hostname() {
        assert_eq!(
            hostname_from_sources(None, None, Some(" etc-host\n".to_owned())),
            Some("etc-host".to_owned())
        );
    }

    #[test]
    fn hostname_ignores_empty_sources() {
        assert_eq!(
            hostname_from_sources(Some(" ".to_owned()), Some("\t".to_owned()), None),
            None
        );
    }
}
