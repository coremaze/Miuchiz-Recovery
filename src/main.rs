#![windows_subsystem = "windows"]
use std::{
    fs,
    path::PathBuf,
    sync::mpsc::{channel, Receiver, Sender},
    thread::{self, sleep},
    time::Duration,
};

use eframe::{egui, emath::Vec2};
use rfd::FileDialog;

mod libmiuchiz_usb;

const FILE_COLOR: egui::Rgba = egui::Rgba::from_rgb(0.1, 0.35, 0.1);
const PAGE_SIZE: usize = 0x1000;
const PAGE_NUM: usize = 512;
const FLASH_FILE_SIZE: usize = PAGE_SIZE * PAGE_NUM;

struct MiuchizApp {
    app_tab: MiuchizAppTab,
    list_rx: Receiver<Vec<PathBuf>>,
    handheld_list: Vec<PathBuf>,
    selected_handheld: Option<PathBuf>,
    selected_load_file: Option<PathBuf>,
    process_rx: Receiver<MiuchizProcess>,
    process_tx: Sender<MiuchizProcess>,
    progress_rx: Receiver<(u32, u32)>,
    text_rx: Receiver<String>,
    text: String,
    processing_state: MiuchizProcess,
    progress_amount: Option<(u32, u32)>,
}
#[derive(Clone, Debug)]
enum MiuchizProcess {
    WatchingForHandhelds,
    Flashing { file: PathBuf, device: PathBuf },
    Reading { file: PathBuf, device: PathBuf },
}

impl Default for MiuchizProcess {
    fn default() -> Self {
        MiuchizProcess::WatchingForHandhelds
    }
}

#[derive(PartialEq)]
enum MiuchizAppTab {
    DumpView,
    FlashView,
}

impl Default for MiuchizAppTab {
    fn default() -> Self {
        Self::DumpView
    }
}

impl MiuchizApp {
    fn new() -> Self {
        // Start a thread for getting handheld list info
        let (list_tx, list_rx) = channel::<Vec<PathBuf>>();
        let (process_tx, process_rx) = channel::<MiuchizProcess>();
        let (process_tx2, process_rx2) = channel::<MiuchizProcess>();
        let (progress_tx, progress_rx) = channel::<(u32, u32)>();
        let (text_tx, text_rx) = channel::<String>();
        thread::spawn(move || {
            Self::process(list_tx, process_rx, process_tx2, progress_tx, text_tx);
        });

        Self {
            app_tab: MiuchizAppTab::default(),
            list_rx,
            handheld_list: Vec::new(),
            selected_handheld: None,
            selected_load_file: None,
            process_rx: process_rx2,
            process_tx,
            progress_rx,
            text_rx,
            text: "OK".to_string(),
            processing_state: MiuchizProcess::default(),
            progress_amount: None,
        }
    }

    fn process(
        list_tx: Sender<Vec<PathBuf>>,
        process_rx: Receiver<MiuchizProcess>,
        process_tx: Sender<MiuchizProcess>,
        progress_tx: Sender<(u32, u32)>,
        text_tx: Sender<String>,
    ) {
        let mut state = MiuchizProcess::default();

        loop {
            if let Ok(new_state) = process_rx.try_recv() {
                state = new_state
            }

            match process_tx.send(state.clone()) {
                Ok(_) => {}
                Err(_) => break,
            }

            match &state {
                MiuchizProcess::WatchingForHandhelds => {
                    let mut new_paths: Vec<PathBuf> = Vec::new();

                    {
                        let set = libmiuchiz_usb::HandheldSet::new();
                        let paths = set.get_handheld_paths();
                        new_paths.reserve(paths.len());

                        for p in paths {
                            new_paths.push(p.to_path_buf());
                        }
                    }

                    match list_tx.send(new_paths) {
                        Ok(_) => {}
                        Err(_) => break,
                    }

                    sleep(Duration::from_millis(250));
                }
                MiuchizProcess::Flashing { file, device } => {
                    text_tx.send("OK".to_string()).ok();
                    if let Err(message) = Self::flash(device, file, &progress_tx, &text_tx) {
                        text_tx.send(message).ok();
                    } else {
                        text_tx
                            .send("Finished writing to handheld.".to_string())
                            .ok();
                    }
                }
                MiuchizProcess::Reading { file, device } => {
                    text_tx.send("OK".to_string()).ok();
                    if let Err(message) = Self::dump(device, file, &progress_tx) {
                        text_tx.send(message).ok();
                    } else {
                        text_tx.send("Finished reading handheld.".to_string()).ok();
                    }
                }
            }

            state = MiuchizProcess::WatchingForHandhelds;
        }
    }

    fn flash(
        device: &PathBuf,
        file: &PathBuf,
        progress_tx: &Sender<(u32, u32)>,
        text_tx: &Sender<String>,
    ) -> Result<(), String> {
        const MAX_PAGE_TIME: Duration = Duration::from_secs(3);
        let data = if let Ok(data) = fs::read(file) {
            data
        } else {
            return Err("Unable to read file.".to_string());
        };

        if data.len() != FLASH_FILE_SIZE {
            return Err(format!(
                "Flash file is not the correct size ({FLASH_FILE_SIZE})."
            ));
        }

        let set = libmiuchiz_usb::HandheldSet::new();

        let mut page: usize = 0;
        while page < PAGE_NUM {
            let page_start = page * PAGE_SIZE;
            let page_end = (page + 1) * PAGE_SIZE;
            let buf = &data.as_slice()[page_start..page_end].to_vec();

            let now = std::time::Instant::now();
            set.write_page(device, page as u32, buf)?;
            let elapsed = now.elapsed();

            // rare
            if elapsed > MAX_PAGE_TIME {
                let new_page = page.checked_sub(1).or(Some(0)).unwrap();
                text_tx
                    .send(format!(
                        "Page {page} took a long time to write. Restarting from {new_page}."
                    ))
                    .ok();
                page = new_page;
                thread::sleep(Duration::from_secs(1));
            } else {
                page += 1;
            }

            progress_tx.send((page as u32, PAGE_NUM as u32)).ok();
        }

        set.eject(device);

        Ok(())
    }

    fn dump(
        device: &PathBuf,
        file: &PathBuf,
        progress_tx: &Sender<(u32, u32)>,
    ) -> Result<(), String> {
        let mut data: Vec<u8> = Vec::new();

        let set = libmiuchiz_usb::HandheldSet::new();
        for page in 0..PAGE_NUM {
            let mut page_data = set.read_page(device, page as u32)?;
            data.append(&mut page_data);

            progress_tx.send((page as u32, PAGE_NUM as u32)).ok();
        }

        if let Err(_) = fs::write(file, data) {
            return Err("Unable to write to file.".to_string());
        }

        Ok(())
    }

    fn update_messages(&mut self) {
        // update handheld list
        if let Ok(message) = self.list_rx.try_recv() {
            self.handheld_list = message
        }

        // Make sure selected handheld is still present
        if let Some(selected_handheld) = &self.selected_handheld {
            let mut selected_handheld_still_valid: bool = false;
            for p in &self.handheld_list {
                if *p == *selected_handheld {
                    selected_handheld_still_valid = true;
                    break;
                }
            }

            if !selected_handheld_still_valid {
                self.selected_handheld = None;
            }
        }

        if let Ok(state) = self.process_rx.try_recv() {
            self.processing_state = state
        }

        if let Ok(progress) = self.progress_rx.try_recv() {
            self.progress_amount = Some(progress);
        }

        if let Ok(text) = self.text_rx.try_recv() {
            self.text = text;
        }
    }

    fn try_set_state(&mut self, process: MiuchizProcess) {
        match &self.processing_state {
            MiuchizProcess::WatchingForHandhelds => {
                self.progress_amount = None;
                self.process_tx.send(process).ok();
            }
            _ => {}
        }
    }

    fn tabs(&mut self, ui: &mut egui::Ui) {
        egui::TopBottomPanel::top("top_tabs_panel")
            .resizable(false)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Tasks");
                    ui.selectable_value(&mut self.app_tab, MiuchizAppTab::DumpView, "Dump");
                    ui.selectable_value(&mut self.app_tab, MiuchizAppTab::FlashView, "Flash");
                });
            });
    }

    fn handhelds_panel(&mut self, ui: &mut egui::Ui) {
        egui::SidePanel::left("left_handhelds_panel")
            .resizable(false)
            .min_width(200.0)
            .show_inside(ui, |ui| {
                ui.heading("Connected handhelds");
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for p in &self.handheld_list {
                            let btn_response = ui.button(p.display().to_string());
                            if btn_response.clicked() {
                                self.selected_handheld = Some(p.to_path_buf());
                            }
                        }
                    });
            });
    }

    fn info_panel(&mut self, ui: &mut egui::Ui) {
        egui::TopBottomPanel::bottom("bottom_info_panel")
            .resizable(false)
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(format!("Status: {}", &self.text));
                });
            });
    }

    fn progress(&mut self, ui: &mut egui::Ui) {
        let (current_progress, max_progress) = if let Some(progress) = self.progress_amount {
            (progress.0, progress.1)
        } else {
            (0, 0)
        };

        let show_details = match self.processing_state {
            MiuchizProcess::WatchingForHandhelds => false,
            MiuchizProcess::Flashing { file: _, device: _ } => true,
            MiuchizProcess::Reading { file: _, device: _ } => true,
        };

        let percentage: f32 = current_progress as f32 / max_progress as f32;
        let text: String = format!("{current_progress}/{max_progress}");
        let mut progress_bar = egui::ProgressBar::new(percentage);

        if show_details {
            progress_bar = progress_bar.show_percentage().text(text);
        }

        ui.add(progress_bar);
    }

    fn selected_handheld_label(&self, ui: &mut egui::Ui) {
        ui.label(format!("Selected handheld: "));
        if let Some(selected_handheld) = &self.selected_handheld {
            ui.colored_label(FILE_COLOR, selected_handheld.display().to_string());
        } else {
            ui.colored_label(egui::Color32::RED, "None");
        }
    }

    fn dump_view(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.vertical_centered_justified(|ui| {
                ui.horizontal(|ui| {
                    self.selected_handheld_label(ui);
                });
                if ui.button("Dump flash").clicked() {
                    if let Some(device) = self.selected_handheld.clone() {
                        if let Some(file) = FileDialog::new().save_file() {
                            self.try_set_state(MiuchizProcess::Reading {
                                file,
                                device: device.to_path_buf(),
                            });
                        }
                    }
                }
                ui.separator();
                self.progress(ui);
            });
        });
    }

    fn flash_view(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.vertical_centered_justified(|ui| {
                ui.horizontal(|ui| {
                    self.selected_handheld_label(ui);
                });
                ui.horizontal(|ui| {
                    ui.label(format!("Selected flash file: "));
                    if let Some(flash_file) = &self.selected_load_file {
                        ui.colored_label(FILE_COLOR, flash_file.display().to_string());
                    } else {
                        ui.colored_label(egui::Color32::RED, "None");
                    }
                });
                if ui.button("Open flash file…").clicked() {
                    if let Some(path) = FileDialog::new().pick_file() {
                        self.selected_load_file = Some(path);
                    }
                }
                if ui.button("Load flash").clicked() {
                    if let (Some(flash_file), Some(device)) = (
                        self.selected_load_file.clone(),
                        self.selected_handheld.clone(),
                    ) {
                        self.try_set_state(MiuchizProcess::Flashing {
                            file: flash_file.to_path_buf(),
                            device: device.to_path_buf(),
                        });
                    }
                }
                ui.separator();
                self.progress(ui);
            });
        });
    }
}

impl eframe::App for MiuchizApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint();
        egui::CentralPanel::default().show(ctx, |ui| {
            self.update_messages();
            self.tabs(ui);
            self.handhelds_panel(ui);
            self.info_panel(ui);

            match self.app_tab {
                MiuchizAppTab::DumpView => self.dump_view(ui),
                MiuchizAppTab::FlashView => self.flash_view(ui),
            }
        });
    }
}

fn main() {
    let window_size = Some(Vec2 { x: 800.0, y: 400.0 });
    eframe::run_native(
        "Miuchiz USB Utility",
        eframe::NativeOptions {
            drag_and_drop_support: true,
            //icon_data: todo!(),
            initial_window_size: window_size,
            min_window_size: window_size,
            max_window_size: window_size,
            resizable: false,
            ..eframe::NativeOptions::default()
        },
        Box::new(|_cc| Box::new(MiuchizApp::new())),
    )
}
