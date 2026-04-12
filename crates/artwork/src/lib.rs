#[derive(Debug, Default, Clone)]
pub struct ArtworkService;

impl ArtworkService {
    pub fn cache_key_for(&self, path: &str) -> String {
        format!("artwork:{path}")
    }
}
