use std::{borrow::Cow, marker::PhantomData};

use tui::{buffer::Buffer, layout::*, style::*, text::*, widgets::*};

pub struct Title<'a> {
    _marker: PhantomData<Box<dyn Fn() + 'a>>,
}

impl Title<'_> {
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

pub struct TitleState<'a> {
    parui: Span<'a>,
    pub query: String,
    old_query: Cow<'a, str>,
    para_line: Vec<Span<'a>>,
    block: Block<'a>,
    pub mod_: Modifier,
    old_mod: Modifier,
    pub col: Color,
    old_col: Color,
    pub size: Rect,
    old_size: Rect,
}

impl<'a> TitleState<'a> {
    pub fn new() -> Self {
        Self {
            parui: Span::raw(" parui "),
            query: String::new(),
            old_query: Cow::Borrowed(""),
            para_line: vec![Span::raw(" Search: "), Span::default()],
            mod_: Modifier::default(),
            old_mod: Modifier::default(),
            col: Color::default(),
            old_col: Color::default(),
            size: Rect::default(),
            old_size: Rect::default(),
            block: Block::default(),
        }
    }
}

impl<'a> StatefulWidget for Title<'a> {
    type State = TitleState<'a>;

    fn render(self, area: Rect, buf: &mut Buffer, s: &mut Self::State) {
        let bold = Style::default().fg(s.col).add_modifier(s.mod_);

        if s.query != s.old_query || s.size.width != s.old_size.width {
            s.para_line[1] = Span::raw(
                s.query
                    .chars()
                    .skip((s.query.len() + 13).saturating_sub(s.size.width as usize))
                    .take(s.size.width.saturating_sub(13) as usize)
                    .collect::<String>(),
            );
        }
        s.para_line[1].style = Style::default().fg(s.col);

        s.para_line[0].style = bold;
        s.parui.style = bold;
        let para = Paragraph::new(Line::from(s.para_line.clone()));

        if s.col != s.old_col || s.mod_ != s.old_mod {
            s.block = Block::default()
                .title(s.parui.clone())
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(s.col));
        }

        para.block(s.block.clone())
            .alignment(Alignment::Left)
            .render(area, buf)
    }
}
