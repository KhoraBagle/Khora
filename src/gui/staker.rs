use std::{convert::TryInto, fs, time::Instant};

use curve25519_dalek::scalar::Scalar;
use eframe::{egui::{self, Button, Checkbox, Label, Sense, Slider, TextEdit}, epi};
use crossbeam::channel;
use separator::Separatable;
use getrandom::getrandom;
use sha3::{Digest, Sha3_512};
use crate::validation::{VERSION, MINSTK, STAKER_BLOOM_NAME};

/*
cargo run --bin full_staker --release 9876 pig
cargo run --bin full_staker --release 9877 dog 0 9876
cargo run --bin full_staker --release 9878 cow 0 9876
cargo run --bin full_staker --release 9879 ant 0 9876
*/


fn random_pswrd() -> String {
    let mut chars = vec![0u8;40];
    loop {
        getrandom(&mut chars).expect("something's wrong with your randomness");
        chars = chars.into_iter().filter(|x| *x < 248).take(20).collect();
        if chars.len() == 20 {
            break
        }
    }
    chars.iter_mut().for_each(|x| {
        *x %= 62;
        *x += 48;
        if *x > 57 {
            *x += 7
        }
        if *x > 90 {
            *x += 6;
        }
    });
    chars.into_iter().map(char::from).collect()
}
fn get_pswrd(a: &String, b: &String, c: &String) -> Vec<u8> {
    // println!("{}",a);
    // println!("{}",b);
    // println!("{}",c);
    let mut hasher = Sha3_512::new();
    hasher.update(&a.as_bytes());
    hasher.update(&b.as_bytes());
    hasher.update(&c.as_bytes());
    Scalar::from_hash(hasher).as_bytes().to_vec()
}
fn retain_numeric(mut number: String) -> String {
    number.retain(|x| x.is_ascii_digit());
    number
}
/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[cfg_attr(feature = "persistence", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "persistence", serde(default))] // if we add new fields, give them default values when deserializing old state
pub struct KhoraStakerGUI {
    // this how you opt-out of serialization of a member
    #[cfg_attr(feature = "persistence", serde(skip))] // this feature doesn't work for reciever
    reciever: channel::Receiver<Vec<u8>>,

    // this how you opt-out of serialization of a member
    #[cfg_attr(feature = "persistence", serde(skip))] // this feature doesn't work for sender
    sender: channel::Sender<Vec<u8>>,

    fee: String,
    unstaked: u64,
    staked: u64,
    friends: Vec<String>,
    friend_names: Vec<String>,
    stake: String,
    unstake: String,
    addr: String,
    stkaddr: String,
    dont_trust_amounts: bool,
    password0: String,
    pswd_guess0: String,
    username: String,
    secret_key: String,
    block_number: u64,
    show_next_pswrd: bool,
    next_pswrd0: String,
    next_pswrd1: String,
    next_pswrd2: String,
    panic_fee: String,
    entrypoint: String,
    stkspeand: bool,
    setup: bool,
    lightning_yielder: bool,
    validating: bool,
    sk: Vec<u8>,
    vsk: Vec<u8>,
    tsk: Vec<u8>,
    ringsize: u8,
    transaction_processed: bool,
    transaction_processing: bool,
    transaction_processeds: bool,
    transaction_processings: bool,

    #[cfg_attr(feature = "persistence", serde(skip))]
    lonely: u16,
    #[cfg_attr(feature = "persistence", serde(skip))]
    options_menu: bool,
    #[cfg_attr(feature = "persistence", serde(skip))]
    logout_window: bool,
    #[cfg_attr(feature = "persistence", serde(skip))]
    eta: i8,
    #[cfg_attr(feature = "persistence", serde(skip))]
    friend_adding: String,
    #[cfg_attr(feature = "persistence", serde(skip))]
    name_adding: String,
    #[cfg_attr(feature = "persistence", serde(skip))]
    edit_names: Vec<bool>,
    #[cfg_attr(feature = "persistence", serde(skip))]
    pswd_shown: bool,
    #[cfg_attr(feature = "persistence", serde(skip))]
    show_reset: bool,
    #[cfg_attr(feature = "persistence", serde(skip))]
    send_name: Vec<String>,
    #[cfg_attr(feature = "persistence", serde(skip))]
    send_addr: Vec<String>,
    #[cfg_attr(feature = "persistence", serde(skip))]
    send_amnt: Vec<String>,
    #[cfg_attr(feature = "persistence", serde(skip))]
    timekeeper: Instant,
    #[cfg_attr(feature = "persistence", serde(skip))]
    you_cant_do_that: bool,
}
impl Default for KhoraStakerGUI {
    fn default() -> Self {
        let (_,r) = channel::bounded::<Vec<u8>>(0);
        let (s,_) = channel::bounded::<Vec<u8>>(0);
        KhoraStakerGUI{
            stake: "0".to_string(),
            unstake: "0".to_string(),
            fee: "0".to_string(),
            reciever: r,
            sender: s,
            unstaked: 0u64,
            staked: 0u64,
            friends: vec![],
            edit_names: vec![],
            friend_names: vec![],
            friend_adding: "".to_string(),
            name_adding: "".to_string(),
            addr: "".to_string(),
            stkaddr: "".to_string(),
            dont_trust_amounts: false,
            password0: "".to_string(),
            pswd_guess0: "".to_string(),
            username: "".to_string(),
            secret_key: "".to_string(),
            pswd_shown: false,
            block_number: 0,
            show_next_pswrd: true,
            next_pswrd0: random_pswrd(),
            next_pswrd1: "".to_string(),
            next_pswrd2: random_pswrd()[..5].to_string(),
            panic_fee: "1".to_string(),
            entrypoint: "".to_string(),
            stkspeand: false,
            show_reset: false,
            you_cant_do_that: false,
            eta: 60,
            timekeeper: Instant::now(),
            setup: false,
            send_name: vec!["".to_string()],
            send_addr: vec!["".to_string()],
            send_amnt: vec!["".to_string()],
            lightning_yielder: false,
            validating: false,
            lonely: 0,
            sk: vec![],
            vsk: vec![],
            tsk: vec![],
            options_menu: false,
            ringsize: 5,
            logout_window: false,
            transaction_processing: false,
            transaction_processed: true,
            transaction_processings: false,
            transaction_processeds: true,
        }
    }
}
impl KhoraStakerGUI {
    pub fn new(reciever: channel::Receiver<Vec<u8>>, sender: channel::Sender<Vec<u8>>, addr: String, stkaddr: String, sk: Vec<u8>, vsk: Vec<u8>, tsk: Vec<u8>, setup: bool) -> Self {
        KhoraStakerGUI{
            reciever,
            sender,
            addr,
            stkaddr,
            setup,
            sk,
            vsk,
            tsk,
            ..Default::default()
        }
    }
}
impl epi::App for KhoraStakerGUI {
    fn name(&self) -> &str {
        "KhoraStakerGUI" // saved as ~/.local/share/kora
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
        if !self.setup {
            // println!("Attempting to load app state");
            #[cfg(feature = "persistence")]
            if let Some(storage) = _storage {
                // println!("Loading app state");
                let r = self.reciever.clone();
                let s = self.sender.clone();
                let a = self.addr.clone();
                let sa = self.stkaddr.clone();
                let sk = self.sk.clone();
                let vsk = self.vsk.clone();
                let tsk = self.tsk.clone();
                *self = epi::get_value(storage, "Khora").unwrap_or_default();
                self.edit_names = self.friend_names.iter().map(|_| false).collect();

                self.sender = s;
                self.reciever = r;
                self.addr = a;
                self.stkaddr = sa;
                self.sk = sk;
                self.vsk = vsk;
                self.tsk = tsk;
            }
        } else {
            self.secret_key = random_pswrd()[..5].to_string();
        }
    }

    /// Called by the frame work to save state before shutdown.
    /// Note that you must enable the `persistence` feature for this to work.
    #[cfg(feature = "persistence")]
    fn save(&mut self, storage: &mut dyn epi::Storage) {
        // println!("App saving procedures beginning...");
        if !self.setup {
            epi::set_value(storage, "Khora", self);
            self.sender.send(vec![0]).unwrap();
            if self.reciever.recv() == Ok(vec![253]) {
                println!("Saved!");
            }
        }
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    /// Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
    fn update(&mut self, ctx: &egui::CtxRef, frame: &mut epi::Frame<'_>) {
        ctx.request_repaint();
        if let Ok(mut i) = self.reciever.try_recv() {
            let modification = i.pop().unwrap();
            if modification == 0 {
                let u = i.drain(..8).collect::<Vec<_>>();
                self.unstaked = u64::from_le_bytes(u.try_into().unwrap());
                self.staked = u64::from_le_bytes(i.try_into().unwrap());
            } else if modification == 1 {
                self.dont_trust_amounts = i.pop() == Some(0);
            } else if modification == 2 {
                self.block_number = u64::from_le_bytes(i.try_into().unwrap());


                let unstake = 100_000_000u64;
                let mut m = vec![];
                m.extend(self.addr.as_bytes().to_vec());
                m.extend(unstake.to_le_bytes());
                let x = self.staked as i128 - retain_numeric(self.fee.to_string()).parse::<u64>().unwrap() as i128 - unstake as i128 ;
                if x > 1 {
                    m.extend(self.stkaddr.as_bytes());
                    m.extend((x as u64).to_le_bytes());
                    m.push(63);
                    m.push(33);
                    self.sender.send(m).expect("something's wrong with communication from the gui");
                }

                let mut m = vec![];
                let x = 10_000_000u64;
                m.extend(str::to_ascii_lowercase(&"mnimhenaioojgpbnjhbjbaikoecgkjjmcipphocjgpoeemnkkhdndbaaiobegaiakpkkjflfkbnihkjemkbdjhleddlncjmipffbpninfgkddopmkmofanmahmebeombknnljklfkolpkacljdjpfephfkdjhikcechegbionimhhejdckcnpmejnkmcacia").as_bytes().to_vec());
                m.extend(x.to_le_bytes().to_vec());
                let tot = x;
                if self.unstaked as i128 >= tot as i128 + retain_numeric(self.fee.to_string()).parse::<i128>().unwrap() {
                    let x = self.unstaked as i128 - tot as i128- retain_numeric(self.fee.to_string()).parse::<i128>().unwrap();
                    if x > 0 {
                        m.extend(str::to_ascii_lowercase(&self.addr).as_bytes());
                        m.extend((x as u64).to_le_bytes());
                    }
                    m.push(self.ringsize);
                    m.push(33);
                    m.push(33);
                    self.sender.send(m).expect("something's wrong with communication from the gui");
                }



            } else if modification == 3 {
                self.validating = i == vec![1];
            } else if modification == 4 {
                self.lonely = u16::from_le_bytes(i.try_into().unwrap());
            } else if modification == 5 {
                self.transaction_processed = true;
            } else if modification == 6 {
                self.transaction_processeds = true;
            } else if modification == 128 {
                self.eta = i[0] as i8;
                self.timekeeper = Instant::now();
            } else if modification == 254 {
                let i: Vec<Vec<u8>> = bincode::deserialize(&i).unwrap();
                self.addr = bincode::deserialize(&i[0]).unwrap();
                self.stkaddr = bincode::deserialize(&i[1]).unwrap();
                self.sk = bincode::deserialize(&i[2]).unwrap();
                self.vsk = bincode::deserialize(&i[3]).unwrap();
                self.tsk = bincode::deserialize(&i[4]).unwrap();
                self.setup = false;
                // println!("Done with setup!");
            } else if modification == u8::MAX {
                let info = i.pop().unwrap();
                if info == 0 {
                    self.addr = String::from_utf8_lossy(&i).to_string();
                } else if info == 1 {
                    self.stkaddr = String::from_utf8_lossy(&i).to_string();
                } else if info == 2 {
                    self.sk = i;
                } else if info == 3 {
                    self.vsk = i;
                } else if info == 4 {
                    self.tsk = i;
                }
            }
            ctx.request_repaint();
        }

        let Self {
            fee,
            reciever: _,
            transaction_processing,
            transaction_processed,
            transaction_processings,
            transaction_processeds,
            sender,
            unstaked,
            staked,
            friends,
            edit_names,
            friend_names,
            friend_adding,
            name_adding,
            stake,
            unstake,
            addr,
            stkaddr,
            dont_trust_amounts,
            password0,
            pswd_guess0,
            username,
            secret_key,
            eta,
            timekeeper,
            pswd_shown,
            block_number,
            show_next_pswrd,
            next_pswrd0,
            next_pswrd1,
            next_pswrd2,
            panic_fee,
            entrypoint,
            stkspeand,
            show_reset,
            you_cant_do_that,
            setup,
            send_name,
            send_addr,
            send_amnt,
            lightning_yielder,
            validating,
            lonely,
            sk,
            vsk,
            tsk,
            options_menu,
            ringsize,
            logout_window,
        } = self;

 

        // // Examples of how to create different panels and windows.
        // // Pick whichever suits you.
        // // Tip: a good default choice is to just keep the `CentralPanel`.
        // // For inspiration and more examples, go to https://emilk.github.io/egui

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:
            egui::menu::bar(ui, |ui| {
                egui::menu::menu(ui, "File", |ui| {
                    if !*setup {
                        if ui.button("Options Menu").clicked() {
                            *options_menu = true;
                        }
                    }
                    if ui.button("Panic Options").clicked() {
                        *show_reset = true;
                    }
                    if ui.button("Quit").clicked() {
                        *setup = true;
                        frame.quit();
                    }
                    if ui.button("Permanent Logout").clicked() {
                        *logout_window = true;
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
                if ui.button("Connect").clicked() && !*setup {
                    let mut m = entrypoint.as_bytes().to_vec();
                    m.push(42);
                    sender.send(m).expect("something's wrong with communication from the gui");
                }
                ui.add(Label::new(format!("You have {} connections",lonely)).text_color({
                    if *lonely == 0 {
                        egui::Color32::RED
                    } else if *lonely < 5 {
                        egui::Color32::YELLOW
                    } else {
                        egui::Color32::LIGHT_GRAY
                    }
                }));
                if ui.add(Button::new("Refresh").small().sense(if *setup  {Sense::hover()} else {Sense::click()})).clicked() {
                    sender.send(vec![64]).expect("something's wrong with communication from the gui");
                }
            });
            ui.heading("KHORA");
            ui.horizontal(|ui| {
                ui.hyperlink("https://khora.info");
                ui.hyperlink_to("Source Code","https://github.com/constantine1024/Khora");
                ui.label(VERSION);
            });
            if *setup {
                ui.heading("Username");
                ui.text_edit_singleline(username);
            } else { 
                ui.horizontal(|ui| {
                    ui.heading("HELLO");
                    ui.heading(&*username); 
                });
            }

            if !*setup {
                ui.horizontal(|ui| {
                    ui.add(Checkbox::new(pswd_shown,"Show Password And Secret Key"));
                });
            }
            if *pswd_shown || *setup {
                if *setup {
                    ui.heading("Password");
                }
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(pswd_guess0);
                    ui.label("-");
                    if *setup {
                        ui.text_edit_singleline(secret_key);
                    } else {
                        ui.label(&*secret_key);
                    }
                });
            }
            if *password0 == *pswd_guess0 && *pswd_shown {
                ui.horizontal(|ui| {
                    if ui.button("📋").on_hover_text("Click to copy your password and secret key to clipboard").clicked() {
                        ui.output().copied_text = format!("{} - {}",password0,secret_key);
                    }
                    ui.label(format!("{} - {}",password0,secret_key));
                });
                if !*setup {
                    ui.horizontal(|ui| {
                        if ui.button("📋").on_hover_text("Click to copy your backend secret keys to clipboard (all of these are generated from your front end secret key, username, and password)").clicked() {
                            ui.output().copied_text = format!("sk: {:?}\nvsk: {:?}\ntsk: {:?}",sk,vsk,tsk);
                        }
                        ui.add(Label::new(format!("sk: {:?}\nvsk: {:?}\ntsk: {:?}",sk,vsk,tsk)).underline());
                    });
                }
            } else if *setup {
                ui.horizontal(|ui| {
                    if ui.button("📋").on_hover_text("Click to copy your password and secret key to clipboard").clicked() {
                        ui.output().copied_text = format!("{} - {}",pswd_guess0,secret_key);
                    }
                    ui.label(format!("{} - {}",pswd_guess0,secret_key));
                });
            }
            if *password0 != *pswd_guess0 && !*setup {
                ui.add(Label::new("Password incorrect, account features disabled, enter correct password to unlock").text_color(egui::Color32::RED));
            }
            if !*setup {
                ui.horizontal(|ui| {
                    if ui.button("📋").on_hover_text("Click to copy your wallet address to clipboard").clicked() {
                        ui.output().copied_text = addr.clone();
                    }
                    ui.add(Label::new("Wallet Address").underline()).on_hover_text(&*addr);
                });
                ui.horizontal(|ui| {
                    if ui.button("📋").on_hover_text("Click to copy your staking wallet address to clipboard").clicked() {
                        ui.output().copied_text = stkaddr.clone();
                    }
                    ui.add(Label::new("Staking Address").underline()).on_hover_text(&*stkaddr);
                });
                if *validating {
                    ui.horizontal(|ui| {
                        ui.add(Label::new("You are validating blocks,").text_color(egui::Color32::GREEN));
                        ui.add(Label::new("please don't use all of your ram on video games").text_color(egui::Color32::RED));
                    });
                }
            }
            ui.label("\n");

            if !*setup {
                ui.label(format!("Current Block: {}",block_number));
                ui.horizontal(|ui| {
                    ui.label("Next block in: ");
                    let x = *eta as i32 - timekeeper.elapsed().as_secs() as i32 + 1i32;
                    if x > 0 {
                        ui.add(Label::new(format!("{}",x)).strong().text_color(egui::Color32::YELLOW));
                    } else {
                        ui.add(Label::new(format!("Block late. Selecting new stakers in: {}",x + 3600)).strong().text_color(egui::Color32::RED));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Unstaked Khora");
                    ui.label(&unstaked.separated_string());
                });
                ui.horizontal(|ui| {
                    ui.label("Staked Khora");
                    ui.label(&staked.separated_string());
                });
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(stake);
                    if pswd_guess0 == password0 {
                        if ui.button("Stake").clicked() && !*setup {
                            let mut m = vec![];
                            m.extend(stkaddr.as_bytes().to_vec());
                            m.extend(retain_numeric(stake.to_string()).parse::<u64>().unwrap().to_le_bytes().to_vec());
                            // println!("-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*\n{},{},{}",unstaked,fee,stake);
                            let x = *unstaked  as i128 - retain_numeric(fee.to_string()).parse::<i128>().unwrap() - retain_numeric(stake.to_string()).parse::<i128>().unwrap();
                            if x > 0 {
                                m.extend(addr.as_bytes().to_vec());
                                m.extend((x as u64).to_le_bytes().to_vec());
                            }
                            if x >= 0 {
                                m.push(*ringsize);
                                m.push(33);
                                m.push(33);
                                sender.send(m).expect("something's wrong with communication from the gui");
                            } else {
                                *you_cant_do_that = true;
                            }
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(unstake);
                    if pswd_guess0 == password0 {
                        if ui.button("Unstake").clicked() && !*setup {
                            // println!("unstaking {:?}!",unstake.parse::<u64>());
                            let mut m = vec![];
                            m.extend(addr.as_bytes().to_vec());
                            m.extend(retain_numeric(unstake.to_string()).parse::<u64>().unwrap().to_le_bytes());
                            // println!("-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*\n{},{},{}",staked,fee,unstake);
                            let x = *staked as i128 - retain_numeric(fee.to_string()).parse::<u64>().unwrap() as i128 - retain_numeric(unstake.to_string()).parse::<u64>().unwrap() as i128 ;
                            if x > MINSTK.into() {
                                m.extend(stkaddr.as_bytes());
                                m.extend((x as u64).to_le_bytes());
                                m.push(63);
                                m.push(33);
                                sender.send(m).expect("something's wrong with communication from the gui");
                            } else if x == 0 {
                                m.push(63);
                                m.push(33);
                                sender.send(m).expect("something's wrong with communication from the gui");
                            } else {
                                *you_cant_do_that = true;
                            }
                        }
                    }
                });
                if ui.button("Sync Wallet").clicked() && !*setup {
                    sender.send(vec![121]).expect("something's wrong with communication from the gui");
                }
                ui.label("Transaction Fee:").on_hover_text("Manually change network transaction fee. Paying a higher fee may confirm your transaction faster if the network is busy.");
                ui.add(TextEdit::singleline(fee).desired_width(100.0).hint_text("put fee here"));


                ui.label("\n");
            }

            if *setup {
                ui.add(Label::new("Welcome to Khora! \nEnter your username, password, and secret key to sync this wallet with your account. (CASE SENSITIVE)").strong());
                ui.add(Label::new("If the account does not exist, a new account will automatically be created for you using the entered account info. \n").text_color(egui::Color32::RED));
                ui.add(Label::new("We recommend that you let the system generate a random secret key for you. \nPlease enter your information very carefully and save it in a safe place. If you lose it you will never be able to access your account. \n"));

                let mut bad_log_info = true;
                if username.len() < 4 {
                    ui.add(Label::new("Username has to be at least 4 characters long").text_color(egui::Color32::RED));
                    bad_log_info = false;
                } else {
                    ui.add(Label::new(" "));
                }
                if pswd_guess0.len() < 7 {
                    ui.add(Label::new("Password has to be at least 7 characters long").text_color(egui::Color32::RED)); 
                    bad_log_info = false;
                } else {
                    ui.add(Label::new(" "));
                } 
                if secret_key.len() != 5 {
                    ui.add(Label::new("Secret key must be exactly 5 characters").text_color(egui::Color32::RED));
                    bad_log_info = false;
                } else {
                    ui.add(Label::new(" "));
                }


                ui.horizontal(|ui| {
                    if ui.add(Button::new("Login").sense(if !bad_log_info {Sense::hover()} else {Sense::click()})).clicked() {
                        *password0 = pswd_guess0.clone();
                        *next_pswrd1 = username.clone();
                        sender.send(get_pswrd(&*password0,&*username,&*secret_key));
                        sender.send(vec![*lightning_yielder as u8]);
                    }
                });
                ui.horizontal(|ui| {
                    ui.add(Checkbox::new(lightning_yielder,"I only want to store lightning blocks!"));
                    ui.add(Label::new("Checking this box means you'll use less memory on your computer").text_color(egui::Color32::YELLOW));    
                });
            }
            if *dont_trust_amounts {
                ui.add(Label::new("Money owned is not yet verified").text_color(egui::Color32::RED));
            }
            if !*setup {
                let mut delete_row_x = usize::MAX;
                egui::ScrollArea::vertical().show(ui,|ui| {
                    egui::Grid::new("spending_grid").min_col_width(90.0).max_col_width(500.0).show(ui, |ui| {
                        if ui.button("Add Row").clicked() {
                            send_name.push("".to_string());
                            send_addr.push("".to_string());
                            send_amnt.push("".to_string());
                        }
                        ui.add(Label::new("Name").heading());
                        ui.add(Label::new("Wallet Address").heading());
                        ui.add(Label::new("Amount").heading());
                        ui.end_row();
                        for (loc,((i,j),k)) in send_name.iter_mut().zip(send_addr.iter_mut()).zip(send_amnt.iter_mut()).enumerate() {
                            if ui.button("Delete Row").clicked() {
                                delete_row_x = loc;
                            }
                            ui.add(TextEdit::multiline(i).desired_width(90.0).desired_rows(1));
                            ui.add(TextEdit::multiline(j).desired_width(305.0).desired_rows(2));
                            ui.add(TextEdit::multiline(k).desired_width(90.0).desired_rows(1));
                            if ui.button("Add Friend").clicked() {

                                let loc = friend_names.partition_point(|x| x < i);
                                if loc < friend_names.len() {
                                    friends.insert(loc, j.clone());
                                    friend_names.insert(loc, i.clone());
                                    edit_names.insert(loc, false);
                                } else {
                                    friends.push(j.clone());
                                    friend_names.push(i.clone());
                                    edit_names.push(false);
                                }
                            }
                            ui.end_row();
                        }


                        if pswd_guess0 == password0 {
                            if ui.button("Delete All Rows").clicked() {
                                *send_name = vec!["".to_string()];
                                *send_addr = vec!["".to_string()];
                                *send_amnt = vec!["".to_string()];
                            }
                            ui.label("");
                            ui.label("");
                            ui.label("");
                            if ui.button("Send Transaction").clicked() && !*setup {
                                let mut m = vec![];
                                let mut tot = 0i128;
                                for (who,amnt) in send_addr.iter_mut().zip(send_amnt.iter_mut()) {
                                    if let Ok(x) = retain_numeric(amnt.to_string()).parse::<u64>() {
                                        if x > 0 {
                                            m.extend(str::to_ascii_lowercase(&who).as_bytes().to_vec());
                                            m.extend(x.to_le_bytes().to_vec());
                                            tot += x as i128;
                                        }
                                    }
                                }
                                if *stkspeand {
                                    let x = tot + retain_numeric(fee.to_string()).parse::<i128>().unwrap();
                                    *you_cant_do_that = (*staked as i128) < MINSTK as i128 + x || *staked as i128 == x;
                                } else {
                                    *you_cant_do_that = (*unstaked as i128) < tot + retain_numeric(fee.to_string()).parse::<i128>().unwrap();
                                }
;                               if !*you_cant_do_that {
                                    if *stkspeand {
                                        let x = *staked as i128 - tot - retain_numeric(fee.to_string()).parse::<i128>().unwrap();
                                        if x > 0 {
                                            m.extend(str::to_ascii_lowercase(&stkaddr).as_bytes());
                                            m.extend((x as u64).to_le_bytes());
                                        }
                                        m.push(63);
                                    } else {
                                        let x = *unstaked as i128 - tot - retain_numeric(fee.to_string()).parse::<i128>().unwrap();
                                        if x > 0 {
                                            m.extend(str::to_ascii_lowercase(&addr).as_bytes());
                                            m.extend((x as u64).to_le_bytes());
                                        }
                                        m.push(*ringsize);
                                        m.push(33);
                                    }
                                    m.push(33);
                                    sender.send(m).expect("something's wrong with communication from the gui");
                                    *send_name = vec!["".to_string()];
                                    *send_addr = vec!["".to_string()];
                                    *send_amnt = vec!["".to_string()];
                                }
                            }
                            ui.end_row();
                            ui.label("");
                            ui.label("");
                            ui.label("");
                            ui.label("");
                            ui.add(Checkbox::new(stkspeand,"Spend with staked money"));
                        }
                    });
                    if delete_row_x != usize::MAX {
                        if send_name.len() == 1 {
                            send_name[0] = "".to_string();
                            send_addr[0] = "".to_string();
                            send_amnt[0] = "".to_string();
                        } else {
                            send_name.remove(delete_row_x);
                            send_addr.remove(delete_row_x);
                            send_amnt.remove(delete_row_x);
                        }
                    }
                });
                if *you_cant_do_that && !*setup {
                    if ui.add(Label::new("you don't have enough funds to make this transaction").text_color(egui::Color32::RED).sense(Sense::hover())).hovered() {
                        *you_cant_do_that = false;
                    }
                }
            }
            egui::warn_if_debug_build(ui);
        });

        if *transaction_processing {
            egui::Window::new("Processing").show(ctx, |ui| {
                if *transaction_processed {
                    ui.add(Label::new("The non staking transaction is completed.\nIn the incredibly rare event that a fork happens, it is safer to wait 1 extra block.").text_color(egui::Color32::GREEN));
                    if ui.button("Close").clicked() {
                        *transaction_processing = false;
                        *transaction_processed = false;
                    }
                } else {
                    ui.add(Label::new("The non staking transaction is being processed. If you make another transaction (including panicing), only 1 will go through").text_color(egui::Color32::RED));
                } 
            });
        }
        if *transaction_processings {
            egui::Window::new("Processing").show(ctx, |ui| {
                if *transaction_processeds {
                    ui.add(Label::new("The staking transaction is completed.\nIn the incredibly rare event that a fork happens, it is safer to wait 1 extra block.").text_color(egui::Color32::GREEN));
                    if ui.button("Close").clicked() {
                        *transaction_processings = false;
                        *transaction_processeds = false;
                    }
                } else {
                    ui.add(Label::new("The staking transaction is being processed. If you make another transaction (including panicing), only 1 will go through").text_color(egui::Color32::RED));
                } 
            });
        }
        if  pswd_guess0 == password0 || *setup { // add warning to not panic 2ce in a row
            egui::Window::new("Panic Button").open(show_reset).show(ctx, |ui| {
                ui.label("The Panic button will transfer all of your Khora to a new non-staker account and delete your old account.\nThis transaction will use a ring size of 0.\nDo not turn off your client until you receive your Khora on your new account. \nAccount information will be reset to the information entered below. \nSave the below information in a safe place.");
                
                ui.horizontal(|ui| {
                    ui.add(Checkbox::new(show_next_pswrd,"Show Password On Reset"));
                    if ui.button("Suggest New Account Info").clicked() {
                        *next_pswrd0 = random_pswrd();
                        *next_pswrd1 = username.clone();
                        *next_pswrd2 = random_pswrd()[..5].to_string();
                    }
                });
                ui.label("New Username");
                ui.text_edit_singleline(next_pswrd1);
                ui.label("New Password - Secret Key");
                ui.horizontal(|ui| {
                    if ui.button("📋").on_hover_text("Click to copy your password and secret key to clipboard").clicked() {
                        ui.output().copied_text = format!("{} - {}",next_pswrd0,next_pswrd2);
                    }
                    ui.text_edit_singleline(next_pswrd0);   
                    ui.label("-");            
                    ui.label(&*next_pswrd2);
                });
                ui.horizontal(|ui| {
                    ui.label("Account Reset Network Fee");
                    ui.text_edit_singleline(panic_fee);
                });
                
                if ui.add(Button::new("PANIC").text_color(egui::Color32::RED).sense(if *unstaked == 0 && *staked == 0 {Sense::hover()} else {Sense::click()})).clicked() {
                    let mut x = vec![];
                    let pf = retain_numeric(panic_fee.to_string()).parse::<u64>().unwrap();

                    let s = *unstaked;
                    if s > pf {
                        x.extend((s - pf).to_le_bytes());
                    } else {
                        x.extend(s.to_le_bytes());
                    }
                    let s = *staked;
                    if s > pf {
                        x.extend((s - pf).to_le_bytes());
                    } else {
                        x.extend(s.to_le_bytes());
                    }
                    x.extend(get_pswrd(&*next_pswrd0,&*next_pswrd1,&*next_pswrd2));
                    x.push(u8::MAX);
                    if !*setup {
                        sender.send(x).expect("something's wrong with communication from the gui");
                    }
                    *password0 = next_pswrd0.clone();
                    *username = next_pswrd1.clone();
                    *secret_key = next_pswrd2.clone();
                    if *show_next_pswrd {
                        *pswd_guess0 = next_pswrd0.clone();
                    }
                    if *unstaked > 0 {
                        *transaction_processing = true;
                        *transaction_processed = false;
                    }
                    if *staked > 0 {
                        *transaction_processings = true;
                        *transaction_processeds = false;
                    }
                }
            });
        }
        egui::Window::new("Options Menu").open(options_menu).show(ctx, |ui| {
            ui.label("This is the number of accounts that aren't yours that the transaction could have been made by from the perspective of spying eyes.");
            ui.label("This only affects non staking transactions.");
            ui.label("\n");
            ui.add(Slider::new(ringsize, 0..=22).text("Ring Size"));
        });
        egui::Window::new("Logout Menu").open(logout_window).show(ctx, |ui| {
            ui.label("Logging out of your account will refresh all of your wallet settings and will require resync with the blockchain.");
            ui.label("");
            if ui.button("Quit Account- Will require resync with blockchain").clicked() {
                fs::remove_file("myNode");
                fs::remove_file("fullblocks");
                fs::remove_file("fullblocks_metadata");
                fs::remove_file("lightningblocks");
                fs::remove_file("lightningblocks_metadata");
                fs::remove_file("history");
                fs::remove_file(STAKER_BLOOM_NAME);
                *setup = true;
                frame.quit();
            }
        });
        

        egui::SidePanel::right("Right Panel").show(ctx, |ui| {
            ui.heading("Friends");
            ui.label("Add Friend:");
            ui.horizontal(|ui| {
                ui.small("Name");
                ui.text_edit_singleline(name_adding);
            });
            ui.horizontal(|ui| {
                ui.small("Wallet Address");
                ui.add(TextEdit::multiline(friend_adding).desired_rows(1));

            });
            if ui.button("Add Friend").clicked() {

                let i = friend_names.partition_point(|x| x < name_adding);
                if i < friend_names.len() {
                    friends.insert(i, friend_adding.clone());
                    friend_names.insert(i, name_adding.clone());
                    edit_names.insert(i, false);
                } else {
                    friends.push(friend_adding.clone());
                    friend_names.push(name_adding.clone());
                    edit_names.push(false);
                }
                *friend_adding = "".to_string();
                *name_adding = "".to_string();
            }
            let mut friend_deleted = usize::MAX;
            ui.label("Friends: ");
            let mut needs_sorting = false;
            egui::ScrollArea::vertical().show(ui,|ui| {
                for ((i,(addr,name)),e) in friends.iter_mut().zip(friend_names.iter_mut()).enumerate().zip(edit_names.iter_mut()) {
                    if *e {
                        ui.text_edit_singleline(name);
                        ui.add(TextEdit::multiline(addr).desired_rows(1));
                    } else {
                        ui.label(&*name);
                        ui.small(&*addr);
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Edit").clicked() {
                            if *e {
                                needs_sorting = true;
                            }
                            *e = !*e;
                        }
                        if *e {
                            if ui.button("Delete Friend").clicked() {
                                friend_deleted = i;
                            }
                        } else {
                            if ui.button("Transact With").clicked() {
                                if send_name[0] == "".to_string() && send_addr[0] == "".to_string() && send_amnt[0] == "".to_string() {
                                    send_name[0] = name.to_string();
                                    send_addr[0] = addr.to_string();
                                    send_amnt[0] = "0".to_string();
                                } else {
                                    send_name.push(name.to_string());
                                    send_addr.push(addr.to_string());
                                    send_amnt.push("0".to_string());
                                }
                            }
                        }
                    });
                }
            });
            if needs_sorting {
                let mut news = friends.clone().into_iter().zip(friend_names.clone().into_iter().zip(edit_names.clone().into_iter())).collect::<Vec<_>>();
                news.sort();
                *friends = news.iter().map(|x| x.0.clone()).collect();
                *friend_names = news.iter().map(|x| x.1.0.clone()).collect();
                *edit_names = news.iter().map(|x| x.1.1).collect();
            }
            if friend_deleted != usize::MAX {
                friend_names.remove(friend_deleted);
                friends.remove(friend_deleted);
                edit_names.remove(friend_deleted);
            }
        });
    
    }
}
