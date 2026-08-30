use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Verdict {
    AC,
    WA,
    TLE,
    MLE,
    RE,
    CE,
    IE,
}

impl Verdict {
    pub fn is_accepted(&self) -> bool {
        matches!(self, Verdict::AC)
    }

    pub fn description(&self) -> &'static str {
        match self {
            Verdict::AC => "Accepted",
            Verdict::WA => "Wrong Answer",
            Verdict::TLE => "Time Limit Exceeded",
            Verdict::MLE => "Memory Limit Exceeded",
            Verdict::RE => "Runtime Error",
            Verdict::CE => "Compile Error",
            Verdict::IE => "Internal Error",
        }
    }

    pub fn status(&self) -> &'static str {
        match self {
            Verdict::AC => "Passed",
            Verdict::WA => "Test Failed",
            Verdict::TLE => "Time Limit Exceeded",
            Verdict::MLE => "Memory Limit Exceeded",
            Verdict::RE => "Runtime Error",
            Verdict::CE => "Compilation Error",
            Verdict::IE => "Internal Error",
        }
    }
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verdict_is_accepted() {
        assert!(Verdict::AC.is_accepted());
        assert!(!Verdict::WA.is_accepted());
        assert!(!Verdict::TLE.is_accepted());
    }

    #[test]
    fn test_verdict_serialization() {
        let verdict = Verdict::AC;
        let json = serde_json::to_string(&verdict).unwrap();
        assert_eq!(json, "\"AC\"");

        let deserialized: Verdict = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, Verdict::AC);
    }
}
