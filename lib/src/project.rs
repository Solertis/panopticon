use std::path::Path;

use program::Program;
use region::{Region,Regions};

#[derive(RustcDecodable,RustcEncodable)]
pub struct Project {
    name: String,
    code: Vec<Program>,
    //data: Vec<Structure>,
    sources: Regions,
    //comments: Vec<Comment>,
}

impl Project {
    fn new(s: String,r: Region) -> Project {
        Project{
            name: s,
            code: Vec::new(),
            sources: Regions::new(r),
        }
    }

    fn open(p: &Path) -> Option<Project> {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use region::Region;

    #[test]
    fn new() {
        let p = Project::new("test".to_string(),Region::undefined("base".to_string(),128));

        assert_eq!(p.name, "test".to_string());
        assert_eq!(p.code, Vec::new());
    }
}
