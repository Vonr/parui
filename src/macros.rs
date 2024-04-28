#[macro_export]
macro_rules! style_inner {
    {$(fg: $fg:expr,)? $(bg: $bg:expr,)? $(mod: $mod:expr,)? $(,)?} => {{
        macro_rules! opt {
            () => {
                None
            };
            ($o:expr) => {
                Some($o)
            };
        }

        macro_rules! opt_mod {
            () => {
                ::tui::style::Modifier::empty()
            };
            ($m:expr) => {
                $m
            };
        }

        Style {
            fg: opt!($($fg)?),
            bg: opt!($($bg)?),
            underline_color: None,
            add_modifier: opt_mod!($($mod)?),
            sub_modifier: ::tui::style::Modifier::empty(),
        }
    }};
}

#[macro_export]
macro_rules! style {
    ($fg:expr) => { $crate::style! { fg: $fg } };

    {$($path:ident: $value:expr),*$(,)?} => {
        $crate::style_inner!($($path: $value,)*)
    };
}

#[macro_export]
macro_rules! cows {
    ($($str:literal),*) => {
        [
            $(Cow::Borrowed($str)),*
        ]
    };
}

#[macro_export]
macro_rules! stream_enter {
    ($stream:expr) => {{
        ::crossterm::execute!(
            $stream,
            ::crossterm::terminal::EnterAlternateScreen,
            ::crossterm::event::EnableMouseCapture,
            ::crossterm::event::EnableBracketedPaste
        )
    }};
}

#[macro_export]
macro_rules! stream_exit {
    ($stream:expr) => {{
        ::crossterm::execute!(
            $stream,
            ::crossterm::terminal::LeaveAlternateScreen,
            ::crossterm::event::DisableMouseCapture,
            ::crossterm::event::DisableBracketedPaste
        )
    }};
}
