// Copyright (C) 2026 mg-chao
// SPDX-License-Identifier: Apache-2.0

//! Native component gallery. Run with `--capture <path.png>` for a rendered frame.

mod button;
mod catalog;
mod shell;

#[cfg(test)]
mod tests;

use std::{path::PathBuf, time::Duration};

use ant_design_gpui::{ConfigScope, Direction, UiRoot, theme::AlgorithmChain};
use gpui::{App, AppContext, Bounds, Focusable, WindowBounds, WindowOptions, px, size};

#[derive(Default)]
struct Options {
    capture: Option<PathBuf>,
    reduced_motion: bool,
    dark: bool,
    compact: bool,
    rtl: bool,
    window_size: Option<(u32, u32)>,
}

impl Options {
    fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut options = Self::default();
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--capture" => {
                    options.capture = Some(
                        arguments
                            .next()
                            .filter(|path| !path.starts_with("--"))
                            .ok_or("--capture requires an output path")?
                            .into(),
                    );
                }
                "--reduced-motion" => options.reduced_motion = true,
                "--dark" => options.dark = true,
                "--compact" => options.compact = true,
                "--rtl" => options.rtl = true,
                "--window-size" => {
                    let value = arguments
                        .next()
                        .ok_or("--window-size requires WIDTHxHEIGHT")?;
                    let (width, height) = value
                        .split_once('x')
                        .ok_or("--window-size requires WIDTHxHEIGHT")?;
                    let dimensions = (width.parse::<u32>(), height.parse::<u32>());
                    let (Ok(width), Ok(height)) = dimensions else {
                        return Err("--window-size requires integer dimensions".into());
                    };
                    if width < 640 || height < 480 {
                        return Err("The minimum gallery window size is 640x480".into());
                    }
                    options.window_size = Some((width, height));
                }
                _ => return Err(format!("unknown argument: {argument}")),
            }
        }
        Ok(options)
    }
}

fn main() {
    let options = Options::parse(std::env::args().skip(1)).unwrap_or_else(|error| {
        eprintln!("{error}\nUsage: gallery [--dark] [--compact] [--rtl] [--reduced-motion] [--window-size WIDTHxHEIGHT] [--capture <path.png>]");
        std::process::exit(2);
    });
    gpui_platform::application().run(move |cx: &mut App| {
        ant_design_gpui::init(cx);
        let scope = ConfigScope::new_window(cx);
        scope
            .update_patch(cx, |patch| {
                patch.motion = Some(!options.reduced_motion);
                patch.direction = Some(if options.rtl {
                    Direction::Rtl
                } else {
                    Direction::Ltr
                });
                patch.theme.get_or_insert_default().algorithms =
                    Some(match (options.dark, options.compact) {
                        (false, false) => AlgorithmChain::default(),
                        (true, false) => AlgorithmChain::dark(),
                        (false, true) => AlgorithmChain::compact(),
                        (true, true) => AlgorithmChain::dark_compact(),
                    });
            })
            .expect("valid gallery configuration");
        let (width, height) = options.window_size.unwrap_or((1280, 900));
        let bounds = Bounds::centered(None, size(px(width as f32), px(height as f32)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(640.), px(480.))),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("Ant Design GPUI · Components");
                let gallery = cx.new(|cx| shell::Gallery::new(scope.clone(), cx));
                let root = cx.new(|cx| UiRoot::new(scope, gallery, window, cx));
                window.focus(&root.focus_handle(cx), cx);
                window.activate_window();
                if let Some(path) = options.capture {
                    window
                        .spawn(cx, async move |cx| {
                            cx.background_executor()
                                .timer(Duration::from_millis(300))
                                .await;
                            let result = cx.update(|window, cx| {
                                window.draw(cx).clear(cx);
                                match window.render_to_image() {
                                    Ok(image) => {
                                        if let Err(error) = image.save(&path) {
                                            eprintln!("Could not save gallery capture: {error}");
                                        }
                                    }
                                    Err(error) => {
                                        eprintln!("Could not render gallery capture: {error}");
                                    }
                                }
                            });
                            if let Err(error) = result {
                                eprintln!("Gallery closed before capture: {error}");
                            }
                        })
                        .detach();
                }
                root
            },
        )
        .expect("open component gallery");
        cx.activate(true);
    });
}
