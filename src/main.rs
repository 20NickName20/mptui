use std::{io::Write, time::{Duration, Instant}};

use anyhow::{Context, Ok};
use crossterm::{QueueableCommand, cursor::MoveTo, event::{Event, KeyCode, KeyEvent}, style::{Attribute, Attributes, Color, Print, SetAttribute, SetAttributes, SetForegroundColor, Stylize}, terminal::{self, Clear, ClearType}};
use mpd::{Client, Song, State};

mod app;
use app::App;

const FULL_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const STATUS_TITLE_POS: MoveTo = MoveTo(7, 4);
const STATUS_LINE_POS: MoveTo = MoveTo(9, 5);
const VOLUME_TITLE_POS: MoveTo = MoveTo(7, 7);
const VOLUME_BAR_POS: MoveTo = MoveTo(9, 8);
const TIME_TITLE_POS: MoveTo = MoveTo(7, 10);
const TIME_BAR_POS: MoveTo = MoveTo(9, 11);
const QUEUE_TITLE_POS: MoveTo = MoveTo(7, 13);
const QUEUE_POS: (u16, u16) = (9, 14);

struct AppData {
    mpc: Client,
    status: mpd::Status,
    queue: Vec<mpd::Song>,
    last_queue_version: u32,
    last_status_update: Instant,
    last_elapsed_time_change: Instant,
    last_elapsed_time: Duration,
    refresh_mpd: bool,
    index: u32,
    volume: i8,

    refresh_screen: bool,
    refresh_status_line: bool,
    refresh_volume_bar: bool,
    refresh_time_bar: bool,
    last_time_bar_refresh: Instant,
    refresh_queue: bool,
    term_size: (u16, u16),
    queue_view_size: u16,
}

type PlayerApp = App<AppData>;

impl AppData {
    fn new(mut mpc: Client) -> anyhow::Result<Self> {
        let status = mpc.status()?;
        let queue = mpc.queue()?;
        let queue_version = status.queue_version;
        let volume = status.volume;
        let term_size = terminal::size()?;
        let queue_view_size = term_size.1 - 17;
        let index = status.song.map(|song| song.pos).unwrap_or(0).saturating_sub(queue_view_size as u32 / 2 - 2);
        let elapsed_time = status.time.map(|t| t.0).unwrap_or(Duration::from_secs(0));
        Ok(Self {
            mpc,
            status,
            queue,
            last_queue_version: queue_version,
            last_status_update: Instant::now(),
            last_elapsed_time_change: Instant::now(),
            last_elapsed_time: elapsed_time,
            refresh_mpd: false,
            index,
            volume,

            refresh_screen: true,
            refresh_status_line: false,
            refresh_volume_bar: false,
            refresh_time_bar: false,
            last_time_bar_refresh: Instant::now(),
            refresh_queue: false,
            term_size,
            queue_view_size,
        })
    }

    fn fetch_mpd_state(&mut self) -> anyhow::Result<()> {
        self.status = self.mpc.status()?;
        self.last_status_update = Instant::now();
        self.refresh_status_line = true;
        self.last_elapsed_time = self.status.time.map(|t| t.0).unwrap_or(Duration::from_secs(0));
        self.last_elapsed_time_change = Instant::now();
        if self.volume != self.status.volume {
            self.volume = self.status.volume;
            self.refresh_volume_bar = true;
        }

        if self.status.queue_version != self.last_queue_version {
            self.queue = self.mpc.queue()?;
            self.last_queue_version = self.status.queue_version;
            self.refresh_queue = true;

            let max = self.queue.len() as u32;
            self.index = if max == 0 { 0 } else { self.index.min(max - 1) };
        }

        self.refresh_mpd = false;
        Ok(())
    }

    fn get_current_song(&self) -> Option<&Song> {
        self.status.song.map(|song| &self.queue[song.pos as usize])
    }

    fn get_total(&self) -> Duration {
        self.status.time.map(|t| t.1).unwrap_or(Duration::from_secs(0))
    }

    fn compute_elapsed(&self) -> Duration {
        match self.status.state {
            State::Play => {
                let since_update = self.last_elapsed_time_change.elapsed();
                (self.last_elapsed_time + since_update).min(self.get_total())
            }
            _ => self.last_elapsed_time,
        }
    }

    fn is_song_ended(&self) -> bool {
        if self.status.state != State::Play {
            return false;
        }
        let total = self.get_total();
        if total == Duration::ZERO {
            return false;
        }
        let since_update = self.last_elapsed_time_change.elapsed();
        let raw_elapsed = self.last_elapsed_time + since_update;
        raw_elapsed >= total
    }

    fn set_volume(&mut self, volume: i8) -> anyhow::Result<()> {
        self.volume = volume.clamp(0, 100);
        self.refresh_volume_bar = true;
        self.mpc.volume(self.volume)?;
        Ok(())
    }

    fn change_volume(&mut self, change: i8) -> anyhow::Result<()> {
        self.set_volume(self.volume + change)?;
        Ok(())
    }

    fn set_pos(&mut self, pos: Duration) -> anyhow::Result<()> {
        if self.status.state == State::Stop {
            return Ok(());
        }
        self.mpc.rewind(pos)?;
        self.last_elapsed_time = pos;
        self.last_elapsed_time_change = Instant::now();
        self.refresh_time_bar = true;
        Ok(())
    }

    fn move_pos(&mut self, seconds: i32) -> anyhow::Result<()> {
        let current = self.compute_elapsed();
        let new_pos = if seconds >= 0 {
            current + Duration::from_secs(seconds as u64)
        } else {
            current.saturating_sub(Duration::from_secs((-seconds) as u64))
        };
        self.set_pos(new_pos)
    }

    fn handle_space(&mut self) -> anyhow::Result<()> {
        if self.status.state == State::Stop {
            self.mpc.play()?;
        } else {
            self.mpc.toggle_pause()?;
        }
        self.refresh_mpd = true;
        Ok(())
    }

    fn next(&mut self) -> anyhow::Result<()> {
        if self.queue.is_empty() {
            return Ok(());
        }
        if self.status.state == State::Stop {
            self.mpc.play()?;
        }
        self.mpc.next()?;
        self.refresh_mpd = true;
        Ok(())
    }

    fn prev(&mut self) -> anyhow::Result<()> {
        if self.queue.is_empty() {
            return Ok(());
        }
        if self.status.state == State::Stop {
            self.mpc.play()?;
        }
        self.mpc.prev()?;
        self.refresh_mpd = true;
        Ok(())
    }

    fn scroll(&mut self, amount: i32) {
        self.index = ((self.index as i32 + amount).max(0) as u32).min(self.status.queue_len);
        self.refresh_queue = true;
    }

    fn focus(&mut self, pos: u32) {
        let total = self.queue_view_size as u32;
        self.index = pos.saturating_sub(total / 2 - 2);
        self.refresh_queue = true;
    }

    fn update(&mut self) -> anyhow::Result<()> {
        if self.last_status_update.elapsed() > FULL_REFRESH_INTERVAL {
            self.refresh_mpd = true;
        }
        if self.is_song_ended() {
            self.refresh_mpd = true;
        }

        if self.refresh_mpd {
            let song_place = self.status.song;
            self.fetch_mpd_state()?;
            if song_place != self.status.song {
                self.refresh_status_line = true;
                self.refresh_time_bar = true;
                self.refresh_queue = true;
                if let Some(place) = self.status.song {
                    if let Some(prev_place) = song_place {
                        self.scroll(place.pos as i32 - prev_place.pos as i32);
                    } else {
                        self.focus(place.pos);
                    }
                }
            }
        }

        Ok(())
    }
}

fn handle_key_press(app: &mut PlayerApp, event: KeyEvent) -> anyhow::Result<()> {
    match event.code {
        KeyCode::Char('Q') | KeyCode::Char('q') => app.exit(),
        KeyCode::Char(' ') => app.data.handle_space()?,
        KeyCode::Char('-') | KeyCode::Char('_') => app.data.change_volume(-5)?,
        KeyCode::Char('+') | KeyCode::Char('=') => app.data.change_volume(5)?,
        KeyCode::Char('n') | KeyCode::Char('N') => app.data.next()?,
        KeyCode::Char('b') | KeyCode::Char('B') => app.data.prev()?,
        KeyCode::Char('g') | KeyCode::Char('G') => {
            if let Some(pos) = app.data.status.song {
                app.data.focus(pos.pos);
            }
        },
        KeyCode::Up => app.data.scroll(-1),
        KeyCode::Down => app.data.scroll(1), 
        KeyCode::Left => app.data.move_pos(-10)?,
        KeyCode::Right => app.data.move_pos(10)?,
        _ => ()
    }
    Ok(())
}

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
    let (width, height) = app.data.term_size;
    if width < 69 || height < 18 {
        app.stdout
            .queue(MoveTo(0, 0))?
            .queue(Print("Terminal is too small!!"))?;
        return Ok(());
    }
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

    app.data.refresh_screen = false;
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
    let width = app.data.term_size.0 - 15;
    if let Some(song) = app.data.get_current_song() {
        let artist = song.artist.as_ref().map(|artist| artist.as_str()).unwrap_or("[Unknown]");
        let title = song.title.as_ref().map(|title| title.as_str()).unwrap_or(song.file.as_str());
        let clear_count = width as usize - 3 - artist.len() - title.len();
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
    app.data.refresh_status_line = false;
    Ok(())
}

fn draw_volume_bar(app: &mut PlayerApp) -> anyhow::Result<()> {
    let total = app.data.term_size.0 as usize - 25;
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

    app.data.refresh_volume_bar = false;
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

    let total = app.data.term_size.0 as usize - 25;
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

    app.data.last_time_bar_refresh = Instant::now();
    app.data.refresh_time_bar = false;
    Ok(())
}

fn draw_queue(app: &mut PlayerApp) -> anyhow::Result<()> {
    let queue_len = app.data.queue.len();
    let start = app.data.index as usize;
    let end = (start + app.data.queue_view_size as usize).min(queue_len);
    let queue_slice = &app.data.queue[start..end];
    let width = app.data.term_size.0 - 15;

    let white = SetForegroundColor(Color::White);
    for (line, song) in queue_slice.into_iter().enumerate() {
        let artist = song.artist.as_ref().map(|artist| artist.as_str()).unwrap_or("[Unknown]");
        let title = song.title.as_ref().map(|title| title.as_str()).unwrap_or(song.file.as_str());
        let duration = song.duration.unwrap_or(Duration::from_secs(0));
        let mut clear_count = width as usize - 11 - artist.len() - title.len();
        app.stdout
            .queue(MoveTo(QUEUE_POS.0, QUEUE_POS.1 + line as u16))?;
        if app.data.status.song == song.place {
            clear_count -= 3;
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
    app.data.refresh_queue = false;
    Ok(())
}

fn render(app: &mut PlayerApp) -> anyhow::Result<()> {
    let mut flush = false;
    if app.data.refresh_screen {
        draw_borders(app)?;
        draw_status_line(app)?;
        draw_volume_bar(app)?;
        draw_time_bar(app)?;
        draw_queue(app)?;
        flush = true;
    }
    if app.data.refresh_status_line {
        draw_status_line(app)?;
        flush = true;
    }
    if app.data.refresh_volume_bar {
        draw_volume_bar(app)?;
        flush = true;
    }
    if app.data.refresh_time_bar || (app.data.status.state == State::Play && app.data.last_time_bar_refresh.elapsed() > Duration::from_millis(500)) {
        draw_time_bar(app)?;
        flush = true;
    }
    if app.data.refresh_queue {
        draw_queue(app)?;
        flush = true;
    }

    if flush {
        app.stdout.flush()?;
    }
    Ok(())
}

fn main_loop(app: &mut PlayerApp) -> anyhow::Result<()> {
    app.data.update()?;

    render(app)?;

    Ok(())
}

fn event_handler(app: &mut PlayerApp, event: Event) -> anyhow::Result<()> {
    match event {
        Event::Key(key_event) if key_event.is_press() => handle_key_press(app, key_event)?,
        Event::Resize(w, h) => {
            app.data.term_size = (w, h);
            app.data.refresh_screen = true;
        },
        _ => ()
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let mpc = Client::connect("127.0.0.1:6600").context("Failed to connect to mpd")?;
    let data = AppData::new(mpc)?;
    let mut app = App::new(data);
    app.main(main_loop, event_handler)
}

