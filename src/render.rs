use crossterm::{QueueableCommand, cursor::MoveTo, style::{Attribute, Attributes, Color, Print, SetAttribute, SetAttributes, SetForegroundColor, Stylize}, terminal::{Clear, ClearType}};
use std::io::Write;
use std::time::{Instant, Duration};
use mpd::State;
use unicode_width::UnicodeWidthStr;

use crate::PlayerApp;

const STATUS_TITLE_POS: MoveTo = MoveTo(7, 4);
const STATUS_LINE_POS: MoveTo = MoveTo(9, 5);
const VOLUME_TITLE_POS: MoveTo = MoveTo(7, 7);
const VOLUME_BAR_POS: MoveTo = MoveTo(9, 8);
const TIME_TITLE_POS: MoveTo = MoveTo(7, 10);
const TIME_BAR_POS: MoveTo = MoveTo(9, 11);
const QUEUE_TITLE_POS: MoveTo = MoveTo(7, 13);
const QUEUE_POS: (u16, u16) = (9, 14);

fn render_block(app: &mut PlayerApp, x0: u16, y0: u16, width: u16, height: u16) -> anyhow::Result<()> {
    let x1 = x0 + width;
    let y1 = y0 + height;
    app.stdout
        .queue(MoveTo(x0, y0))?
        .queue(Print("┌"))?
        .queue(Print("╌".repeat(width as usize - 2)))?
        .queue(Print("┐"))?;

    for y in (y0 + 1)..(y1 - 1) {
        app.stdout
            .queue(MoveTo(x0, y))?
            .queue(Print("╎"))?
            .queue(MoveTo(x1 - 1, y))?
            .queue(Print("╎"))?;
    }

    app.stdout
        .queue(MoveTo(x0, y1 - 1))?
        .queue(Print("└"))?
        .queue(Print("╌".repeat(width as usize - 2)))?
        .queue(Print("┘"))?;

    Ok(())
}

fn draw_borders(app: &mut PlayerApp) -> anyhow::Result<()> {
    let (width, height) = app.term_size;
    let color = Color::AnsiValue(92);
    let block_color = Color::AnsiValue(248);
    app.stdout
        .queue(Clear(ClearType::All))?
        .queue(MoveTo(1, 0))?
        .queue(SetForegroundColor(color))?
        .queue(Print("╓"))?
        .queue(Print("─".repeat(width as usize - 4)))?
        .queue(Print("╖"))?

        .queue(MoveTo(1, 1))?
        .queue(Print("║"))?
        .queue(MoveTo(width - 2, 1))?
        .queue(Print("║"))?

        .queue(MoveTo(1, 2))?
        .queue(Print("╠"))?
        .queue(Print("═".repeat(width as usize - 4)))?
        .queue(Print("╣"))?

        .queue(MoveTo(1, 3))?
        .queue(Print("║ ┌"))?
        .queue(Print("─".repeat(width as usize - 8)))?
        .queue(Print("┐ ║"))?;

    for y in 4..(height - 2) {
        app.stdout
            .queue(MoveTo(1, y))?
            .queue(Print("║ │"))?
            .queue(MoveTo(width - 4, y))?
            .queue(Print("│ ║"))?;
    }

    app.stdout
        .queue(MoveTo(1, height - 2))?
        .queue(Print("║ └"))?
        .queue(Print("─".repeat(width as usize - 8)))?
        .queue(Print("┘ ║"))?

        .queue(MoveTo(1, height - 1))?
        .queue(Print("╚"))?
        .queue(Print("═".repeat(width as usize - 4)))?
        .queue(Print("╝"))?

        .queue(SetForegroundColor(block_color))?;

    render_block(app, 5, 4, width - 10, 3)?;
    render_block(app, 5, 7, width - 10, 3)?;
    render_block(app, 5, 10, width - 10, 3)?;
    render_block(app, 5, 13, width - 10, height - 15)?;
    app.stdout
        .queue(VOLUME_TITLE_POS)?
        .queue(SetForegroundColor(Color::Green))?
        .queue(Print("  Volume "))?
        .queue(QUEUE_TITLE_POS)?
        .queue(SetForegroundColor(Color::White))?
        .queue(Print(" 󰕲 Queue "))?;
    app.data.queue_view_size = height - 17;

    app.data.redraw_screen = false;
    Ok(())
}

fn draw_status_line(app: &mut PlayerApp) -> anyhow::Result<()> {
    let state_text = match app.data.status.state {
        State::Stop =>  "  Stopped ".red(),
        State::Pause => "   Paused ".yellow(),
        State::Play =>  "  Playing ".green()
    };
    app.stdout
        .queue(STATUS_TITLE_POS)?
        .queue(Print(state_text))?
        .queue(STATUS_LINE_POS)?;
    let width = app.term_size.0 - 15;
    if let Some(song) = app.data.get_current_song() {
        let artist = song.artist.as_deref().unwrap_or("[Unknown]");
        let title = song.title.as_deref().unwrap_or(song.file.as_str());
        let text_width = UnicodeWidthStr::width(artist) + 3 + UnicodeWidthStr::width(title);
        let clear_count = (width as usize).saturating_sub(text_width);
        app.stdout
            .queue(SetAttribute(Attribute::Bold))?
            .queue(SetForegroundColor(Color::Yellow))?
            .queue(Print(artist))?
            .queue(SetForegroundColor(Color::White))?
            .queue(Print(" - "))?
            .queue(SetForegroundColor(Color::AnsiValue(117)))?
            .queue(Print(title))?
            .queue(SetAttribute(Attribute::Reset))?
            .queue(Print(" ".repeat(clear_count)))?;
    } else {
        let clear_count = width as usize - 8;
        app.stdout
            .queue(SetAttribute(Attribute::Italic))?
            .queue(SetForegroundColor(Color::White))?
            .queue(Print(" No song"))?
            .queue(SetAttribute(Attribute::Reset))?
            .queue(Print(" ".repeat(clear_count)))?;
    }
    app.data.redraw_status_line = false;
    Ok(())
}

fn draw_volume_bar(app: &mut PlayerApp) -> anyhow::Result<()> {
    let total = app.term_size.0 as usize - 25;
    let filled = total * app.data.volume as usize / 100;
    let unfilled = total - filled;
    app.stdout
        .queue(VOLUME_BAR_POS)?
        .queue(SetForegroundColor(Color::White))?
        .queue(Print(format!("{: >3}% ", app.data.volume)))?
        .queue(SetForegroundColor(Color::Green))?
        .queue(Print("["))?
        .queue(Print("#".repeat(filled)))?
        .queue(Print("-".repeat(unfilled)))?
        .queue(Print("]"))?;

    app.data.redraw_volume_bar = false;
    Ok(())
}

fn fmt_duration(duration: Duration) -> String {
    let time = duration.as_secs();
    let seconds = time % 60;
    let minutes = time / 60;
    format!("{:0>2}:{:0>2}", minutes, seconds)
}

fn draw_time_bar(app: &mut PlayerApp) -> anyhow::Result<()> {
    let total_time = app.data.get_total();
    let elapsed = app.data.compute_elapsed().min(total_time);

    app.stdout
        .queue(TIME_TITLE_POS)?
        .queue(SetForegroundColor(Color::White))?
        .queue(Print(" ⏱ "))?
        .queue(SetForegroundColor(Color::Magenta))?
        .queue(Print(fmt_duration(elapsed)))?
        .queue(SetForegroundColor(Color::White))?
        .queue(Print(" / "))?
        .queue(SetForegroundColor(Color::Magenta))?
        .queue(Print(fmt_duration(total_time)))?;

    let total = app.term_size.0 as usize - 25;
    let filled = total * elapsed.as_secs() as usize / total_time.as_secs().max(1) as usize;
    let percent = filled * 100 / total;
    let unfilled = total - filled;
    app.stdout
        .queue(TIME_BAR_POS)?
        .queue(SetForegroundColor(Color::White))?
        .queue(Print(format!("{: >3}% ", percent)))?
        .queue(SetForegroundColor(Color::Magenta))?
        .queue(Print("["))?
        .queue(Print("#".repeat(filled)))?
        .queue(Print("-".repeat(unfilled)))?
        .queue(Print("]"))?;

    app.data.last_time_bar_redraw = Instant::now();
    app.data.redraw_time_bar = false;
    Ok(())
}

fn draw_queue(app: &mut PlayerApp) -> anyhow::Result<()> {
    let queue_len = app.data.queue.len();
    let start = app.data.index as usize;
    let end = (start + app.data.queue_view_size as usize).min(queue_len);
    let queue_slice = &app.data.queue[start..end];
    let width = app.term_size.0 - 15;

    let white = SetForegroundColor(Color::White);
    for (line, song) in queue_slice.iter().enumerate() {
        let artist = song.artist.as_deref().unwrap_or("[Unknown]");
        let title = song.title.as_deref().unwrap_or(song.file.as_str());
        let duration = song.duration.unwrap_or(Duration::from_secs(0));
        let text_width = UnicodeWidthStr::width(artist) + UnicodeWidthStr::width(title) + 11;
        let mut clear_count = (width as usize).saturating_sub(text_width);
        app.stdout
            .queue(MoveTo(QUEUE_POS.0, QUEUE_POS.1 + line as u16))?;
        if app.data.status.song == song.place {
            clear_count = clear_count.saturating_sub(3);
            let attr = Attributes::none().with(Attribute::Bold).with(Attribute::Underlined);
            app.stdout
                .queue(SetAttributes(attr))?
                .queue(white)?
                .queue(Print(" 󰶻 "))?;
        }
        app.stdout
            .queue(SetForegroundColor(Color::Yellow))?
            .queue(Print(artist))?
            .queue(white)?
            .queue(Print(" - "))?
            .queue(SetForegroundColor(Color::AnsiValue(117)))?
            .queue(Print(title))?
            .queue(white)?
            .queue(Print(" ["))?
            .queue(SetForegroundColor(Color::Magenta))?
            .queue(Print(fmt_duration(duration)))?
            .queue(white)?
            .queue(Print("]"))?
            .queue(SetAttribute(Attribute::Reset))?
            .queue(Print(" ".repeat(clear_count)))?;
    }
    let clear_str = " ".repeat(width as usize);
    for line in queue_slice.len()..(app.data.queue_view_size as usize) {
        app.stdout
            .queue(MoveTo(QUEUE_POS.0, QUEUE_POS.1 + line as u16))?
            .queue(Print(&clear_str))?;
    }
    app.data.redraw_queue = false;
    Ok(())
}

pub fn render(app: &mut PlayerApp) -> anyhow::Result<()> {
    let (width, height) = app.term_size;
    if width < 69 || height < 18 {
        app.stdout
            .queue(Clear(ClearType::All))?
            .queue(MoveTo(0, 0))?
            .queue(Print("Terminal is too small!"))?
            .flush()?;
        return Ok(());
    }

    let mut flush = false;
    if app.data.redraw_screen {
        draw_borders(app)?;
        draw_status_line(app)?;
        draw_volume_bar(app)?;
        draw_time_bar(app)?;
        draw_queue(app)?;
        flush = true;
    }
    if app.data.redraw_status_line {
        draw_status_line(app)?;
        flush = true;
    }
    if app.data.redraw_volume_bar {
        draw_volume_bar(app)?;
        flush = true;
    }
    if app.data.redraw_time_bar || (app.data.status.state == State::Play && app.data.last_time_bar_redraw.elapsed() > Duration::from_millis(500)) {
        draw_time_bar(app)?;
        flush = true;
    }
    if app.data.redraw_queue {
        draw_queue(app)?;
        flush = true;
    }

    if flush {
        app.stdout.flush()?;
    }
    Ok(())
}
