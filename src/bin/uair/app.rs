use std::io::{self, Write, Error as IoError, ErrorKind};
use std::fs;
use std::time::{Duration, Instant};
use uair::{Command, PauseArgs, ResumeArgs};
use futures_lite::FutureExt;
use crate::{Args, Error};
use crate::config::{Config, ConfigBuilder};
use crate::socket::Listener;
use crate::session::SessionId;
use crate::timer::UairTimer;

pub struct App {
	listener: Listener,
	sid: SessionId,
	config: Config,
	timer: UairTimer,
	started: Instant,
}

impl App {
	pub fn new(args: Args) -> Result<Self, Error> {
		let conf_data = match fs::read_to_string(&args.config) {
			Ok(c) => c,
			Err(_) => return Err(Error::IoError(IoError::new(
				ErrorKind::NotFound,
				format!("Could not load config file \"{}\"", args.config),
			))),
		};
		let config = ConfigBuilder::deserialize(&conf_data)?.build();
		Ok(App {
			listener: Listener::new(&args.socket)?,
			sid: SessionId::new(&config.sessions, config.iterations),
			config,
			timer: UairTimer::new(Duration::default(), Duration::from_secs(1)),
			started: Instant::now(),
		})
	}

	pub async fn run(mut self) -> Result<(), Error> {
		let mut stdout = io::stdout();
		write!(stdout, "{}", self.config.startup_text)?;
		stdout.flush()?;

		let session = &self.config.sessions[self.sid.curr()];
		self.timer.duration = session.duration;
		let mut state = if self.config.pause_at_start || !session.autostart
			{ State::Paused } else { State::Resumed };

		loop {
			match state {
				State::Paused => state = self.pause_session().await?,
				State::Resumed => {
					self.started = Instant::now();
					state = self.run_session().await?;
					if let State::Paused = state {
						self.timer.duration -= Instant::now() - self.started;
					}
				}
				State::Finished => break,
				State::Reset(sid) => {
					self.sid = sid;
					let session = &self.config.sessions[self.sid.curr()];
					self.timer = UairTimer::new(session.duration, Duration::from_secs(1));
					state = if session.autostart { State::Resumed } else { State::Paused };
				}
			}
		}
		Ok(())
	}

	async fn run_session(&self) -> Result<State, Error> {
		let session = &self.config.sessions[self.sid.curr()];

		match self.timer.start(session, self.started).or(self.handle_commands::<true>()).await? {
			Event::Finished => {
				session.run_command()?;
				if self.sid.is_last() {
					Ok(State::Finished)
				} else {
					Ok(State::Reset(self.sid.next()))
				}
			}
			Event::Command(Command::Pause(_)) => Ok(State::Paused),
			Event::Command(Command::Next(_)) => Ok(State::Reset(self.sid.next())),
			Event::Command(Command::Prev(_)) => Ok(State::Reset(self.sid.prev())),
			_ => unreachable!(),
		}
	}

	async fn pause_session(&self) -> Result<State, Error> {
		let mut stdout = io::stdout();
		let session = &self.config.sessions[self.sid.curr()];

		write!(
			stdout, "{}",
			session.display::<false>(self.timer.duration + Duration::from_secs(1))
		)?;
		stdout.flush()?;

		match self.handle_commands::<false>().await? {
			Event::Command(Command::Resume(_)) => {
				write!(
					stdout, "{}",
					session.display::<true>(self.timer.duration + Duration::from_secs(1))
				)?;
				stdout.flush()?;
				Ok(State::Resumed)
			}
			Event::Command(Command::Next(_)) => Ok(State::Reset(self.sid.next())),
			Event::Command(Command::Prev(_)) => Ok(State::Reset(self.sid.prev())),
			_ => unreachable!(),
		}
	}

	async fn handle_commands<const R: bool>(&self) -> Result<Event, Error> {
		loop {
			let mut stream = self.listener.listen().await?;
			let msg = stream.read().await?;
			let command: Command = bincode::deserialize(&msg)?;
			match command {
				Command::Pause(_) | Command::Toggle(_) if R =>
					return Ok(Event::Command(Command::Pause(PauseArgs {}))),
				Command::Resume(_) | Command::Toggle(_) if !R =>
					return Ok(Event::Command(Command::Resume(ResumeArgs {}))),
				Command::Next(_) if !self.sid.is_last() =>
					return Ok(Event::Command(command)),
				Command::Prev(_) if !self.sid.is_first() =>
					return Ok(Event::Command(command)),
				_ => {}
			}
		}
	}
}

pub enum Event {
	Command(Command),
	Finished,
}

enum State {
	Paused,
	Resumed,
	Reset(SessionId),
	Finished,
}

#[cfg(test)]
mod tests {
	use crate::{app::App, Args};

	#[test]
	fn indicate_missing_config_file() {
		let result = App::new(Args {
			config: "~/.config/uair/no_uair.toml".into(),
			socket: "/tmp/uair.sock".into(),
		});
		assert_eq!(
			result.err().unwrap().to_string(),
			"IO Error: Could not load config file \"~/.config/uair/no_uair.toml\"",
		);
	}
}
