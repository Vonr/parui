use bytemuck::NoUninit;

#[derive(Clone, Copy, NoUninit)]
#[repr(u8)]
pub enum Mode {
    Insert,
    Select,
}
