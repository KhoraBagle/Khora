use eframe::{egui::{self, Button, Checkbox, Label, Sense, TextEdit}, epi};







fn main() {


    // creates the setup screen (sets the values used in the loops and sets some gui options)
    let app = TestGUI::default();
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(Box::new(app), native_options);
}
/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[cfg_attr(feature = "persistence", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "persistence", serde(default))] // if we add new fields, give them default values when deserializing old state
#[derive(Default, Clone, Debug)]
pub struct TestGUI {
    entrypoint: String,
}
impl epi::App for TestGUI {
    fn name(&self) -> &str {
        "GUItesting" // saved as ~/.local/share/kora
    }

    /// Called once before the first frame.
    fn setup(
        &mut self,
        _ctx: &egui::CtxRef,
        _frame: &mut epi::Frame<'_>,
        _storage: Option<&dyn epi::Storage>,
    ) {
        // println!("This is printing before the first frame!");
        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        println!("Attempting to load app state");
        #[cfg(feature = "persistence")]
        if let Some(storage) = _storage {
            println!("Loading app state");
            *self = epi::get_value(storage, "Khora").unwrap_or_default();
        }
    }

    /// Called by the frame work to save state before shutdown.
    /// Note that you must enable the `persistence` feature for this to work.
    #[cfg(feature = "persistence")]
    fn save(&mut self, storage: &mut dyn epi::Storage) {
        println!("App saving procedures beginning...");
        epi::set_value(storage, "Khora", self);
        println!("App saved!");
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    /// Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
    fn update(&mut self, ctx: &egui::CtxRef, frame: &mut epi::Frame<'_>) {
        // ctx.request_repaint();
        println!("updating frame");
        let Self {
            entrypoint,
        } = self;

 

        // // Examples of how to create different panels and windows.
        // // Pick whichever suits you.
        // // Tip: a good default choice is to just keep the `CentralPanel`.
        // // For inspiration and more examples, go to https://emilk.github.io/egui

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:
            egui::menu::bar(ui, |ui| {
                egui::menu::menu(ui, "File", |ui| {
                    if ui.button("Options Menu").clicked() {
                        // *options_menu = true;
                    }
                    if ui.button("Panic Options").clicked() {
                        // *show_reset = true;
                    }
                    if ui.button("Quit").clicked() {
                        frame.quit();
                    }
                    if ui.button("Permanent Logout").clicked() {
                        // *logout_window = true;
                    }
                });
            });
            // egui::util::undoer::default(); // there's some undo button
        });

        egui::CentralPanel::default().show(ctx, |ui| { 
            ui.horizontal(|ui| {
                ui.label("Mesh Network Gate IP");
                ui.add(TextEdit::singleline(entrypoint).desired_width(100.0).hint_text("put entry here"));
                ui.add(Label::new(":8334").text_color(egui::Color32::LIGHT_GRAY));
                if ui.button("Connect").clicked() {
                    
                }
                if ui.add(Button::new("Refresh").small().sense(Sense::click())).clicked() {
                    // sender.send(vec![64]).expect("something's wrong with communication from the gui");
                }
            });
            ui.heading("KHORA");
            ui.horizontal(|ui| {
                ui.hyperlink("https://khora.info");
                ui.hyperlink_to("Source Code","https://github.com/constantine1024/Khora");
                // ui.label(VERSION);
            });

        });

        // println!("done updating");
    
    }
}
