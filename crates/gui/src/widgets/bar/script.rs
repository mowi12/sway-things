// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The basic rows of an item that shows a command of the user's: the program (typed or chosen
//! as a file) and its arguments, one per line, which become waybar's `exec` with each argument
//! quoted, so none can become shell syntax; how often it runs (every few seconds with what that
//! costs, or a line per update, or on a signal); whether it prints plain text or JSON; and
//! _Test_, which runs it once.
//!
//! Nothing runs unless the user presses _Test_. The test runs the program with its arguments as
//! an argument list, never through a shell, with no input and a time limit, off the main loop,
//! and shows what came out: the exit code, how long it took, what it printed and the fields of
//! its JSON, with a warning where it takes longer than it runs every. A command that uses the
//! shell (a pipe, a variable) can't be split into a program and arguments: it stays text under
//! _Advanced_, and isn't run by the test.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use serde_json::Value as Json;
use sway_things_core::bar::script::{self, TestError, TestRun};
use sway_things_core::bar::tilde;

use super::dialog::Form;
use crate::i18n::{Msg, msg, tr, trf};
use crate::style;

/// The cost of running every so many seconds.
const COST: Msg = msg!("Runs {count} times a minute.");
/// The cost, where it adds up.
const COST_HIGH: Msg =
    msg!("Runs {count} times a minute: a slow command there costs battery and makes the bar lag.");
/// A command that uses the shell.
const SHELL: Msg = msg!(
    "This command uses the shell: {command}. Change it under Advanced; the test runs only a \
     program with its arguments, so it is off here."
);
/// What a test run gave.
const EXIT: Msg = msg!("Exit code {code} after {time}");
/// A test run that was ended.
const ENDED: Msg = msg!("Ended after {time}: it ran longer than the test waits");
/// A test that waited for the first line.
const FIRST_LINE: Msg = msg!("Its first line came after {time}; the bar shows a line per update");
/// A run longer than its interval.
const SLOW: Msg =
    msg!("It took longer ({time}) than it runs every ({interval} s): waybar would lag behind.");
/// The program wasn't found.
const NOT_FOUND: Msg = msg!("Program not found: {program}. Give its full path, or install it.");
/// The file can't be run.
const NOT_EXECUTABLE: Msg =
    msg!("{file} can't be run: it isn't executable. Make it executable (chmod +x) first.");
/// It couldn't start.
const NOT_STARTED: Msg = msg!("It couldn't be started: {error}");
/// The JSON fields.
const JSON_FIELDS: Msg = msg!("JSON: {fields}");
/// Output that isn't JSON waybar reads.
const NOT_JSON: Msg = msg!("Not JSON waybar reads: {error}");
/// The heading of what it printed.
const PRINTED: Msg = msg!("It printed:");
/// The heading of its errors.
const ERRORS: Msg = msg!("Errors:");
/// While it runs.
const RUNNING: Msg = msg!("Running…");
/// Nothing printed.
const NOTHING_PRINTED: Msg = msg!("It printed nothing.");

/// How a command runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Every few seconds (`interval`).
    Every,
    /// Once, printing a line per update (no `interval`).
    Lines,
    /// On a signal (`signal`, `interval` `once`).
    Signal,
}

/// Adds the rows of a command item to `content`; they fill in the fields of `form`.
#[expect(
    clippy::too_many_lines,
    reason = "the rows of one form, read best in order"
)]
pub fn basic(content: &gtk::Box, form: &Rc<Form>) {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let exec = form.text("exec");
    let words = if exec.trim().is_empty() {
        Some(Vec::new())
    } else {
        script::split(&exec, home.as_deref())
    };
    let list = crate::widgets::list::section_list(tr(msg!("Command")));
    list.add_css_class("st-bar-basic");
    let program = gtk::Entry::new();
    program.add_css_class("monospace");
    program.set_hexpand(true);
    program.set_placeholder_text(Some(tr(msg!("A program or the path of a script"))));
    program.update_property(&[gtk::accessible::Property::Label(tr(msg!("Program")))]);
    let choose = gtk::Button::with_label(tr(msg!("Choose…")));
    choose.update_property(&[gtk::accessible::Property::Label(tr(msg!(
        "Choose the program's file"
    )))]);
    let program_line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    program_line.append(&program);
    program_line.append(&choose);
    let arguments = gtk::TextBuffer::new(None);
    let arguments_view = gtk::TextView::with_buffer(&arguments);
    arguments_view.set_monospace(true);
    arguments_view.set_size_request(-1, 64);
    arguments_view.update_property(&[gtk::accessible::Property::Label(tr(msg!(
        "Arguments, one per line"
    )))]);
    let frame = gtk::Frame::new(None);
    frame.set_child(Some(&arguments_view));
    if let Some((first, rest)) = words.as_ref().and_then(|words| words.split_first()) {
        program.set_text(first);
        arguments.set_text(&rest.join("\n"));
    }
    let editable = words.is_some();
    program_line.set_sensitive(editable);
    frame.set_sensitive(editable);
    list.append(&crate::widgets::list::list_row(&stacked(
        tr(msg!("Program")),
        &program_line,
    )));
    list.append(&crate::widgets::list::list_row(&stacked(
        tr(msg!("Arguments, one per line")),
        &frame,
    )));
    if !editable {
        let note = gtk::Label::new(Some(&trf(SHELL, &[("command", &exec)])));
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        list.append(&crate::widgets::list::list_row(&note));
    }
    // The words the rows hold, into `exec`.
    let sync = {
        let weak = Rc::downgrade(form);
        let program = program.downgrade();
        let arguments = arguments.downgrade();
        move || {
            let (Some(form), Some(program), Some(arguments)) =
                (weak.upgrade(), program.upgrade(), arguments.upgrade())
            else {
                return;
            };
            form.set_text("exec", &script::join(&words_of(&program, &arguments)));
        }
    };
    let on_program = sync.clone();
    program.connect_changed(move |_| on_program());
    let on_arguments = sync.clone();
    arguments.connect_changed(move |_| on_arguments());
    let chosen = program.downgrade();
    choose.connect_clicked(move |button| {
        let dialog = gtk::FileDialog::builder()
            .title(tr(msg!("Choose the Program")))
            .modal(true)
            .build();
        let window = button.root().and_downcast::<gtk::Window>();
        let chosen = chosen.clone();
        dialog.open(window.as_ref(), gio::Cancellable::NONE, move |file| {
            if let (Ok(file), Some(program)) = (file, chosen.upgrade()) {
                if let Some(path) = file.path() {
                    program.set_text(&path.display().to_string());
                }
            }
        });
    });

    let mode = gtk::DropDown::from_strings(&[
        tr(msg!("Every few seconds")),
        tr(msg!("Once; it prints a line per update")),
        tr(msg!("When it gets a signal")),
    ]);
    let initial =
        if !form.text("signal").is_empty() || form.other("interval") == Some(Json::from("once")) {
            Mode::Signal
        } else if form.text("interval").is_empty() && !exec.trim().is_empty() {
            Mode::Lines
        } else {
            Mode::Every
        };
    mode.set_selected(match initial {
        Mode::Every => 0,
        Mode::Lines => 1,
        Mode::Signal => 2,
    });
    let seconds = gtk::SpinButton::with_range(1.0, 86_400.0, 1.0);
    seconds.set_value(form.text("interval").parse::<f64>().unwrap_or(10.0));
    seconds.update_property(&[gtk::accessible::Property::Label(tr(msg!("Seconds")))]);
    let signal = gtk::SpinButton::with_range(1.0, 30.0, 1.0);
    signal.set_value(form.text("signal").parse::<f64>().unwrap_or(8.0));
    signal.update_property(&[gtk::accessible::Property::Label(tr(msg!(
        "Signal (RTMIN+n)"
    )))]);
    let cost = gtk::Label::new(None);
    cost.set_xalign(0.0);
    cost.set_wrap(true);
    cost.add_css_class(style::DIM);
    cost.add_css_class("st-row-detail");
    let runs = gtk::Box::new(gtk::Orientation::Vertical, 6);
    runs.add_css_class("st-bar-basic-row");
    let runs_line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let runs_title = gtk::Label::new(Some(tr(msg!("Runs"))));
    runs_title.set_xalign(0.0);
    runs_title.set_hexpand(true);
    mode.update_property(&[gtk::accessible::Property::Label(tr(msg!("Runs")))]);
    runs_line.append(&runs_title);
    runs_line.append(&mode);
    runs_line.append(&seconds);
    runs_line.append(&signal);
    runs.append(&runs_line);
    runs.append(&cost);
    list.append(&crate::widgets::list::list_row(&runs));
    let apply_mode = {
        let weak = Rc::downgrade(form);
        let (mode, seconds, signal, cost) = (
            mode.downgrade(),
            seconds.downgrade(),
            signal.downgrade(),
            cost.downgrade(),
        );
        move || {
            let (Some(form), Some(mode), Some(seconds), Some(signal), Some(cost)) = (
                weak.upgrade(),
                mode.upgrade(),
                seconds.upgrade(),
                signal.upgrade(),
                cost.upgrade(),
            ) else {
                return;
            };
            let chosen = mode_of(&mode);
            seconds.set_visible(chosen == Mode::Every);
            signal.set_visible(chosen == Mode::Signal);
            match chosen {
                Mode::Every => {
                    let every = u64::try_from(seconds.value_as_int()).unwrap_or(1).max(1);
                    form.set_text("interval", &every.to_string());
                    form.set_text("signal", "");
                    form.set_other("interval", None);
                    let count = script::runs_a_minute(every);
                    cost.set_text(&trf(
                        if every <= 2 { COST_HIGH } else { COST },
                        &[("count", &count)],
                    ));
                    cost.set_visible(true);
                }
                Mode::Lines => {
                    form.set_text("interval", "");
                    form.set_text("signal", "");
                    form.set_other("interval", None);
                    cost.set_visible(false);
                }
                Mode::Signal => {
                    form.set_text("interval", "");
                    form.set_text("signal", &signal.value_as_int().to_string());
                    form.set_other("interval", Some(Json::from("once")));
                    cost.set_text(&trf(
                        msg!("Runs once at the start, then on: pkill -RTMIN+{signal} waybar"),
                        &[("signal", &signal.value_as_int())],
                    ));
                    cost.set_visible(true);
                }
            }
        }
    };
    apply_mode();
    let on_mode = apply_mode.clone();
    mode.connect_selected_notify(move |_| on_mode());
    let on_seconds = apply_mode.clone();
    seconds.connect_value_changed(move |_| on_seconds());
    signal.connect_value_changed(move |_| apply_mode());

    let output = gtk::DropDown::from_strings(&[
        tr(msg!("Plain text")),
        tr(msg!("JSON: text, tooltip, class")),
    ]);
    output.set_selected(u32::from(
        form.choice("return-type") == Some(Json::from("json")),
    ));
    let weak = Rc::downgrade(form);
    output.connect_selected_notify(move |output| {
        if let Some(form) = weak.upgrade() {
            let json = (output.selected() == 1).then(|| Json::from("json"));
            form.set_choice("return-type", json.as_ref());
        }
    });
    let output_line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    output_line.add_css_class("st-bar-basic-row");
    let output_title = gtk::Label::new(Some(tr(msg!("It prints"))));
    output_title.set_xalign(0.0);
    output_title.set_hexpand(true);
    output.update_property(&[gtk::accessible::Property::Label(tr(msg!("It prints")))]);
    output_line.append(&output_title);
    output_line.append(&output);
    list.append(&crate::widgets::list::list_row(&output_line));

    let test = gtk::Button::with_label(tr(msg!("Test")));
    test.set_tooltip_text(Some(tr(msg!(
        "Runs it once now, without a shell, and shows what it prints"
    ))));
    test.set_sensitive(editable);
    let result = gtk::Label::new(None);
    result.set_xalign(0.0);
    result.set_wrap(true);
    result.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    result.set_selectable(true);
    result.add_css_class("monospace");
    result.add_css_class("st-bar-test-result");
    result.set_visible(false);
    let test_line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let test_note = gtk::Label::new(Some(tr(msg!(
        "Waybar runs this command, never Sway Things: only Test does, once."
    ))));
    test_note.set_xalign(0.0);
    test_note.set_wrap(true);
    test_note.set_hexpand(true);
    test_note.add_css_class(style::DIM);
    test_note.add_css_class("st-row-detail");
    test_line.append(&test_note);
    test_line.append(&test);
    let test_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    test_box.add_css_class("st-bar-basic-row");
    test_box.append(&test_line);
    test_box.append(&result);
    list.append(&crate::widgets::list::list_row(&test_box));
    let (program, arguments) = (program.downgrade(), arguments.downgrade());
    let (mode, seconds, output) = (mode.downgrade(), seconds.downgrade(), output.downgrade());
    let shown = result.downgrade();
    test.connect_clicked(move |button| {
        let (Some(program), Some(arguments), Some(mode), Some(seconds), Some(output), Some(result)) = (
            program.upgrade(),
            arguments.upgrade(),
            mode.upgrade(),
            seconds.upgrade(),
            output.upgrade(),
            shown.upgrade(),
        ) else {
            return;
        };
        let words = words_of(&program, &arguments);
        let chosen = mode_of(&mode);
        let interval = (chosen == Mode::Every)
            .then(|| u64::try_from(seconds.value_as_int()).ok())
            .flatten();
        let json = output.selected() == 1;
        let lines = chosen == Mode::Lines;
        let limit = Duration::from_secs(if lines { 5 } else { 10 });
        button.set_sensitive(false);
        result.set_visible(true);
        result.set_text(tr(RUNNING));
        let button = button.downgrade();
        let result = result.downgrade();
        glib::spawn_future_local(async move {
            let ran = gio::spawn_blocking(move || script::test(&words, limit, lines, json)).await;
            if let Some(button) = button.upgrade() {
                button.set_sensitive(true);
            }
            let Some(result) = result.upgrade() else { return };
            let said = match ran {
                Ok(ran) => report(ran, interval, lines),
                Err(_) => tr(msg!("The test couldn't run.")).to_owned(),
            };
            result.set_text(&said);
            result.announce(&said, gtk::AccessibleAnnouncementPriority::Medium);
        });
    });
    content.append(&list);
}

/// `title` above `widget`, as a row.
fn stacked(title: &str, widget: &impl IsA<gtk::Widget>) -> gtk::Box {
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    let row = gtk::Box::new(gtk::Orientation::Vertical, 6);
    row.add_css_class("st-bar-basic-row");
    row.append(&label);
    row.append(widget);
    row
}

/// The program and its arguments: the program, then each line that isn't empty.
fn words_of(program: &gtk::Entry, arguments: &gtk::TextBuffer) -> Vec<String> {
    let program = program.text().trim().to_owned();
    if program.is_empty() {
        return Vec::new();
    }
    let text = arguments.text(&arguments.start_iter(), &arguments.end_iter(), false);
    std::iter::once(program)
        .chain(
            text.lines()
                .filter(|line| !line.is_empty())
                .map(str::to_owned),
        )
        .collect()
}

/// The mode `dropdown` has picked.
fn mode_of(dropdown: &gtk::DropDown) -> Mode {
    match dropdown.selected() {
        1 => Mode::Lines,
        2 => Mode::Signal,
        _ => Mode::Every,
    }
}

/// A time as the result shows it.
fn time(duration: Duration) -> String {
    if duration < Duration::from_secs(1) {
        format!("{} ms", duration.as_millis())
    } else {
        format!("{:.1} s", duration.as_secs_f64())
    }
}

/// What a test run says.
fn report(ran: Result<TestRun, TestError>, interval: Option<u64>, lines: bool) -> String {
    let run = match ran {
        Ok(run) => run,
        Err(TestError::Empty) => return tr(msg!("Give a program first.")).to_owned(),
        Err(TestError::NotFound(program)) => return trf(NOT_FOUND, &[("program", &program)]),
        Err(TestError::NotExecutable(file)) => {
            return trf(NOT_EXECUTABLE, &[("file", &tilde(&file))]);
        }
        Err(TestError::Start(error)) => return trf(NOT_STARTED, &[("error", &error)]),
    };
    let mut parts = Vec::new();
    let took = time(run.took);
    parts.push(match run.code {
        Some(code) => trf(EXIT, &[("code", &code), ("time", &took)]),
        None if lines && !run.timed_out => trf(FIRST_LINE, &[("time", &took)]),
        None => trf(ENDED, &[("time", &took)]),
    });
    if let Some(interval) = interval {
        if run.took > Duration::from_secs(interval) {
            parts.push(trf(SLOW, &[("time", &took), ("interval", &interval)]));
        }
    }
    let printed: Vec<&str> = run.stdout.lines().take(20).collect();
    if printed.is_empty() {
        parts.push(tr(NOTHING_PRINTED).to_owned());
    } else {
        parts.push(format!("{}\n{}", tr(PRINTED), printed.join("\n")));
    }
    let errors: Vec<&str> = run.stderr.lines().take(10).collect();
    if !errors.is_empty() {
        parts.push(format!("{}\n{}", tr(ERRORS), errors.join("\n")));
    }
    match run.json {
        Some(Ok(fields)) => {
            let fields: Vec<String> = fields
                .iter()
                .map(|(key, value)| format!("{key} = {value}"))
                .collect();
            parts.push(trf(JSON_FIELDS, &[("fields", &fields.join(", "))]));
        }
        Some(Err(error)) => parts.push(trf(NOT_JSON, &[("error", &error)])),
        None => {}
    }
    parts.join("\n\n")
}
