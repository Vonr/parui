use bytemuck::NoUninit;

#[derive(Clone, Copy, NoUninit, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    Insert,
    Select,
}
