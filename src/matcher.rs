pub trait Matcher<T>
where
    T: ?Sized,
{
    fn matches(&self, value: T) -> bool;
}

impl<T> Matcher<T> for str
where
    T: AsRef<str>,
{
    fn matches(&self, value: T) -> bool {
        self.is_empty() || value.as_ref().contains(self)
    }
}

impl<T> Matcher<T> for regex::Regex
where
    T: AsRef<str>,
{
    fn matches(&self, value: T) -> bool {
        self.is_match(value.as_ref())
    }
}
