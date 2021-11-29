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
    value: bool,
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
            *self = epi::get_value(storage, self.name()).unwrap_or_default();
        }
    }

    /// Called by the frame work to save state before shutdown.
    /// Note that you must enable the `persistence` feature for this to work.
    #[cfg(feature = "persistence")]
    fn save(&mut self, storage: &mut dyn epi::Storage) {
        println!("App saving procedures beginning...");
        epi::set_value(storage, self.name(), self);
        println!("App saved!");
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    /// Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
    fn update(&mut self, ctx: &egui::CtxRef, frame: &mut epi::Frame<'_>) {
        // ctx.request_repaint();
        println!("updating frame");
        let Self {
            value,
        } = self;

 

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                egui::menu::menu(ui, "File", |ui| {
                    if ui.button("Quit").clicked() {
                        frame.quit();
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| { 
            ui.heading("GUI TESTER");
            ui.horizontal(|ui| {
                ui.hyperlink("https://google.com");
            });

        });

    
    }
}
