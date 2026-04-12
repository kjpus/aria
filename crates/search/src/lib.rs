#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub id: String,
    pub title: String,
    pub kind: String,
}

#[derive(Debug, Default, Clone)]
pub struct SearchService;

impl SearchService {
    pub fn query(&self, _query: &SearchQuery) -> Vec<SearchHit> {
        Vec::new()
    }
}
