pub(super) fn client_environment_assignments() -> Vec<String> {
    let mut assignments = std::env::vars()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>();
    assignments.sort_unstable();
    assignments
}

#[cfg(test)]
mod tests {
    use super::client_environment_assignments;

    #[test]
    fn client_environment_assignments_are_name_value_pairs() {
        let assignments = client_environment_assignments();

        assert!(assignments.iter().all(|value| value.contains('=')));
    }

    #[test]
    fn client_environment_assignments_are_stably_ordered() {
        let assignments = client_environment_assignments();
        let mut sorted = assignments.clone();
        sorted.sort_unstable();

        assert_eq!(assignments, sorted);
    }
}
