use serde::{Deserialize, Serialize};

use crate::models::generic::MaybeRfc2822DateTime;

#[derive(Debug, Deserialize, Serialize)]
pub struct StatusJSON {
    pub last_snapshot: MaybeRfc2822DateTime,
    pub last_gc: MaybeRfc2822DateTime,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::generic::Rfc2822DateTime;

    #[test]
    fn test_status_json_deserialization() {
        let json_data = r#"
        {
            "last_snapshot": "Fri Jun 21 17:40:02 UTC 2024",
            "last_gc": "Sun Jun 16 00:00:59 UTC 2024"
        }
        "#;

        let status: StatusJSON = serde_json::from_str(json_data).unwrap();

        // Wed, 18 Feb 2015 23:16:09 GMT
        assert_eq!(
            status.last_snapshot.try_into_datetime().unwrap(),
            Some(
                Rfc2822DateTime::from("Fri, 21 Jun 2024 17:40:02 +0000")
                    .try_into()
                    .unwrap()
            )
        );
        assert_eq!(
            status.last_gc.try_into_datetime().unwrap(),
            Some(
                Rfc2822DateTime::from("Sun, 16 Jun 2024 00:00:59 +0000")
                    .try_into()
                    .unwrap()
            )
        );
    }
}
