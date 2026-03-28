use std::{io::{Stdout, stdout}, time::{Duration, Instant}, thread};
use crossterm::{cursor, event::{self, Event, KeyCode, KeyModifiers}, execute, terminal};

const UPDATES_PER_SECOND: u64 = 10;
const FRAME_DELAY: Duration = Duration::from_nanos(1_000_000_000u64 / UPDATES_PER_SECOND);

pub struct App<T> {
    should_exit: bool,
    pub data: T,
    pub stdout: Stdout,
    pub term_size: (u16, u16)
}

impl<T> App<T> {
    pub fn new(data: T) -> anyhow::Result<App<T>> {
        let term_size = terminal::size()?;
        Ok(App::<T> {
            should_exit: false,
            data: data,
            stdout: stdout(),
            term_size
        })
    }

    pub fn exit(&mut self) {
        self.should_exit = true;
    }

    fn poll_events(&mut self, event_handler: fn(&mut App<T>, Event) -> anyhow::Result<()>) -> anyhow::Result<()> {
        while event::poll(Duration::from_secs(0)).unwrap_or(false) {
            let event = event::read()?;
            match event {
                Event::Key(key) if key.is_press() && key.modifiers == KeyModifiers::CONTROL && (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('q')) => self.exit(),
                Event::Resize(w, h) => self.term_size = (w, h),
                _ => ()
            }
            event_handler(self, event)?;
        }
        Ok(())
    }

    pub fn main(&mut self, action: fn(&mut App<T>) -> anyhow::Result<()>, event_handler: fn(&mut App<T>, Event) -> anyhow::Result<()>) -> anyhow::Result<()> {
        execute!(
            self.stdout,
            crossterm::terminal::EnterAlternateScreen,
            cursor::Hide
        ).unwrap();
        crossterm::terminal::enable_raw_mode().expect("Unable to enable raw mode");



        while !self.should_exit {
            let frame_start = Instant::now();

            self.poll_events(event_handler)?;

            action(self)?;

            let elapsed = frame_start.elapsed();
            if elapsed < FRAME_DELAY {
                thread::sleep(FRAME_DELAY - elapsed);
            }
        }

        crossterm::terminal::disable_raw_mode().unwrap();
        execute!(
            self.stdout,
            crossterm::terminal::LeaveAlternateScreen,
            cursor::Show
        ).unwrap();

        Ok(())
    }
}

