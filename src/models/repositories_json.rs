use serde::{Deserialize, Serialize};

use super::generic::MaybeRfc2822DateTime;

// The format of the repositories.json also includes the metadata for the server:
// {
//     "schema" : 1,
//     "last_geodb_update" : "Tue Jun 18 13:40:04 UTC 2024",
//     "cvmfs_version" : "2.11.3-1",
//     "os_id" : "rhel",
//     "os_version_id" : "9.4",
//     "os_pretty_name" : "Red Hat Enterprise Linux 9.4 (Plow)",
//     "repositories" : [
//     ],
//     "replicas" : [
//       {
//         "name"  : "dev.eessi.io",
//         "url"   : "/cvmfs/dev.eessi.io"
//       },
//       {
//         "name"  : "riscv.eessi.io",
//         "url"   : "/cvmfs/riscv.eessi.io"
//       },
//       {
//         "name"  : "software.eessi.io",
//         "url"   : "/cvmfs/software.eessi.io"
//       }
//     ]
//   }

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RepositoriesJSON {
    pub schema: u32,
    pub last_geodb_update: MaybeRfc2822DateTime,
    pub cvmfs_version: Option<String>,
    pub os_id: Option<String>,
    pub os_version_id: Option<String>,
    pub os_pretty_name: Option<String>,
    pub repositories: Vec<RepositoriesJSONRepo>,
    pub replicas: Vec<RepositoriesJSONRepo>,
}

impl RepositoriesJSON {
    /// Returns a list of repositories and replicas.
    ///
    /// This function returns a list of repositories and replicas. It is a convenience function
    /// that combines the `repositories` and `replicas` fields into a single list.
    pub fn repositories_and_replicas(&self) -> Vec<RepositoriesJSONRepo> {
        let mut repos = self.repositories.clone();
        repos.extend(self.replicas.clone());
        repos
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RepositoriesJSONRepo {
    pub name: String,
    pub url: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::generic::Rfc2822DateTime;

    #[test]
    fn test_repositories_json_deserialization() {
        let json_data = r#"
        {
            "schema": 1,
            "last_geodb_update": "Tue Jun 18 13:40:04 UTC 2024",
            "cvmfs_version": "2.11.3-1",
            "os_id": "rhel",
            "os_version_id": "9.4",
            "os_pretty_name": "Red Hat Enterprise Linux 9.4 (Plow)",
            "repositories": [],
            "replicas": [
                {
                    "name": "dev.eessi.io",
                    "url": "/cvmfs/dev.eessi.io"
                },
                {
                    "name": "riscv.eessi.io",
                    "url": "/cvmfs/riscv.eessi.io"
                },
                {
                    "name": "software.eessi.io",
                    "url": "/cvmfs/software.eessi.io"
                }
            ]
        }
        "#;
        // Fri, 21 Jun 2024 17:40:02 +0000
        let metadata: RepositoriesJSON = serde_json::from_str(json_data).unwrap();
        assert_eq!(metadata.schema, 1);
        assert_eq!(
            metadata.last_geodb_update.try_into_datetime().unwrap(),
            Some(
                Rfc2822DateTime::from("Tue, 18 Jun 2024 13:40:04 +0000")
                    .try_into()
                    .unwrap()
            )
        );
        assert_eq!(metadata.cvmfs_version, Some("2.11.3-1".to_string()));
        assert_eq!(metadata.os_id, Some("rhel".to_string()));
        assert_eq!(metadata.os_version_id, Some("9.4".to_string()));
        assert_eq!(
            metadata.os_pretty_name,
            Some("Red Hat Enterprise Linux 9.4 (Plow)".to_string())
        );
        assert_eq!(metadata.repositories.len(), 0);
        assert_eq!(metadata.replicas.len(), 3);

        assert_eq!(metadata.replicas[0].name, "dev.eessi.io");
        assert_eq!(metadata.replicas[0].url, "/cvmfs/dev.eessi.io");

        assert_eq!(metadata.replicas[1].name, "riscv.eessi.io");
        assert_eq!(metadata.replicas[1].url, "/cvmfs/riscv.eessi.io");

        assert_eq!(metadata.replicas[2].name, "software.eessi.io");
        assert_eq!(metadata.replicas[2].url, "/cvmfs/software.eessi.io");
    }

    #[test]
    fn test_repositories_and_replicas_with_replicas() {
        let json_data = r#"
        {
            "schema": 1,
            "last_geodb_update": "Tue Jun 18 13:40:04 UTC 2024",
            "cvmfs_version": "2.11.3-1",
            "os_id": "rhel",
            "os_version_id": "9.4",
            "os_pretty_name": "Red Hat Enterprise Linux 9.4 (Plow)",
            "repositories": [],
            "replicas": [
                {
                    "name": "replica1",
                    "url": "/cvmfs/replica1"
                },
                {
                    "name": "replica2",
                    "url": "/cvmfs/replica2"
                }
            ]
        }
        "#;

        let metadata: RepositoriesJSON = serde_json::from_str(json_data).unwrap();
        let repos = metadata.repositories_and_replicas();
        assert_eq!(repos.len(), 2);

        assert_eq!(repos[0].name, "replica1");
        assert_eq!(repos[0].url, "/cvmfs/replica1");

        assert_eq!(repos[1].name, "replica2");
        assert_eq!(repos[1].url, "/cvmfs/replica2");
    }

    #[test]
    fn test_repositories_and_replicas_with_repositories() {
        let json_data = r#"
        {
            "schema": 1,
            "last_geodb_update": "Tue Jun 18 13:40:04 UTC 2024",
            "cvmfs_version": "2.11.3-1",
            "os_id": "rhel",
            "os_version_id": "9.4",
            "os_pretty_name": "Red Hat Enterprise Linux 9.4 (Plow)",
            "repositories": [
                {
                    "name": "repo1",
                    "url": "/cvmfs/repo1"
                },
                {
                    "name": "repo2",
                    "url": "/cvmfs/repo2"
                }
            ],
            "replicas": []
        }
        "#;

        let metadata: RepositoriesJSON = serde_json::from_str(json_data).unwrap();
        let repos = metadata.repositories_and_replicas();
        assert_eq!(repos.len(), 2);

        assert_eq!(repos[0].name, "repo1");
        assert_eq!(repos[0].url, "/cvmfs/repo1");

        assert_eq!(repos[1].name, "repo2");
        assert_eq!(repos[1].url, "/cvmfs/repo2");
    }

    #[test]
    fn test_repositories_and_replicas_without_repositories_and_replicas() {
        let json_data = r#"
        {
            "schema": 1,
            "last_geodb_update": "Tue Jun 18 13:40:04 UTC 2024",
            "cvmfs_version": "2.11.3-1",
            "os_id": "rhel",
            "os_version_id": "9.4",
            "os_pretty_name": "Red Hat Enterprise Linux 9.4 (Plow)",
            "repositories": [],
            "replicas": []
        }
        "#;

        let metadata: RepositoriesJSON = serde_json::from_str(json_data).unwrap();
        let repos = metadata.repositories_and_replicas();
        assert_eq!(repos.len(), 0);
    }

    #[test]
    fn test_last_geodb_update_incorrect_date_format() {
        let json_data = r#"
        {
            "schema": 1,
            "last_geodb_update": "Tue Jun 18 13:40:04 2024",
            "cvmfs_version": "2.11.3-1",
            "os_id": "rhel",
            "os_version_id": "9.4",
            "os_pretty_name": "Red Hat Enterprise Linux 9.4 (Plow)",
            "repositories": [],
            "replicas": []
        }
        "#;

        let metadata: RepositoriesJSON = serde_json::from_str(json_data).unwrap();
        assert!(metadata.last_geodb_update.try_into_datetime().is_err());
    }
}
