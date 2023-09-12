use bytemuck::NoUninit;

#[derive(Clone, Copy, NoUninit)]
#[repr(u8)]
pub enum Message {
    TrySearch,
    ListingPackages,
    Searching,
    NoResults,
}

impl Message {
    pub(crate) const fn as_str(&self) -> &'static str {
        match self {
            Message::TrySearch => "Try searching for something",
            Message::ListingPackages => "Listing packages...",
            Message::Searching => "Searching for packages...",
            Message::NoResults => "No results, try another query",
        }
    }
}
