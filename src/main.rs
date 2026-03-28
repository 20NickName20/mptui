use std::time::{Duration, Instant};
use crossterm::event::{Event, KeyCode, KeyEvent};
use crossterm::terminal;

use anyhow::{Context, Ok};
use mpd::{Client, Song, State};

mod app;
use app::App;

mod render;

const FULL_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

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

    redraw_screen: bool,
    redraw_status_line: bool,
    redraw_volume_bar: bool,
    redraw_time_bar: bool,
    last_time_bar_redraw: Instant,
    redraw_queue: bool,
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
        let queue_view_size = term_size.1.saturating_sub(17);
        let index = status.song.map(|song| song.pos).unwrap_or(0).saturating_sub((queue_view_size as u32 / 2).saturating_sub(2));
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

            redraw_screen: true,
            redraw_status_line: false,
            redraw_volume_bar: false,
            redraw_time_bar: false,
            last_time_bar_redraw: Instant::now(),
            redraw_queue: false,
            queue_view_size,
        })
    }

    fn fetch_mpd_state(&mut self) -> anyhow::Result<()> {
        self.status = self.mpc.status()?;
        self.last_status_update = Instant::now();
        self.redraw_status_line = true;
        self.last_elapsed_time = self.status.time.map(|t| t.0).unwrap_or(Duration::from_secs(0));
        self.last_elapsed_time_change = Instant::now();
        if self.volume != self.status.volume {
            self.volume = self.status.volume;
            self.redraw_volume_bar = true;
        }

        if self.status.queue_version != self.last_queue_version {
            self.queue = self.mpc.queue()?;
            self.last_queue_version = self.status.queue_version;
            self.redraw_queue = true;

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
        self.redraw_volume_bar = true;
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
        self.redraw_time_bar = true;
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
        self.redraw_queue = true;
    }

    fn focus(&mut self, pos: u32) {
        let total = self.queue_view_size as u32;
        self.index = pos.saturating_sub(total / 2 - 2);
        self.redraw_queue = true;
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
                self.redraw_status_line = true;
                self.redraw_time_bar = true;
                self.redraw_queue = true;
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
        KeyCode::Char(c) if c.is_ascii_digit() => {
            if let Some(duration) = app.data.status.duration {
                let part = c.to_digit(10).unwrap();
                app.data.set_pos(duration * part / 10)?;
            }
        },
        _ => ()
    }
    Ok(())
}


fn main_loop(app: &mut PlayerApp) -> anyhow::Result<()> {
    app.data.update()?;

    render::render(app)?;

    Ok(())
}

fn event_handler(app: &mut PlayerApp, event: Event) -> anyhow::Result<()> {
    match event {
        Event::Key(key_event) if key_event.is_press() => handle_key_press(app, key_event)?,
        Event::Resize(_, _) => {
            app.data.redraw_screen = true;
        },
        _ => ()
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let mpc = Client::connect("127.0.0.1:6600").context("Failed to connect to mpd")?;
    let data = AppData::new(mpc)?;
    let mut app = App::new(data)?;
    app.main(main_loop, event_handler)
}

