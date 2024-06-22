use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Debug)]
pub struct MetaJSON {
    pub administrator: String,
    pub email: String,
    pub organisation: String,
    pub custom: Value,
}

/* fn main() {
    let json_data = r#"
    {
        "administrator": "EESSI CVMFS Administrators",
        "email": "support@eessi.io",
        "organisation": "EESSI",
        "custom": {
            "_comment": "See https://eessi.io/docs/ for more information about the EESSI repository."
        }
    }
    "#;

    let repo_info: RepositoryInfo = serde_json::from_str(json_data).unwrap();
    println!("{:?}", repo_info);
} */
