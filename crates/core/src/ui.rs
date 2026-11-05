use crate::params::MacroScale;

#[derive(Debug, Clone)]
pub enum UiCommand {
    Label(String),
    Heading(String),
    Separator,
    HorizontalSeparator,
    VerticalSeparator,
    Space(f32),
    RadioParam {
        index: usize,
        label: String,
        choices: Vec<String>,
    },
    Button {
        id: String,
        label: String,
        clicked: bool,
    },
    Param {
        index: usize,
        label: String,
        scale: Option<MacroScale>,
    },
    Meter {
        value: f32,
        label: Option<String>,
    },
    Row(Vec<UiCommand>),
    Column(Vec<UiCommand>),
}

enum Frame {
    Root(Vec<UiCommand>),
    Row(Vec<UiCommand>),
    Column(Vec<UiCommand>),
}

pub struct UiBuilder {
    stack: Vec<Frame>,
    clicked: Vec<String>,
}

impl Default for UiBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl UiBuilder {
    pub fn new() -> Self {
        Self {
            stack: vec![Frame::Root(Vec::new())],
            clicked: Vec::new(),
        }
    }

    pub fn begin(&mut self, clicked: Vec<String>) {
        self.stack.clear();
        self.stack.push(Frame::Root(Vec::new()));
        self.clicked = clicked;
    }

    pub fn finish(&mut self) -> Result<Vec<UiCommand>, &'static str> {
        if self.stack.len() != 1 {
            return Err("unbalanced ui.row()/ui.column() calls");
        }

        match self.stack.pop() {
            Some(Frame::Root(commands)) => Ok(commands),
            _ => Err("unbalanced ui.row()/ui.column() calls"),
        }
    }

    pub fn label(&mut self, text: String) {
        self.push(UiCommand::Label(text));
    }

    pub fn heading(&mut self, text: String) {
        self.push(UiCommand::Heading(text));
    }

    pub fn separator(&mut self) {
        self.push(UiCommand::Separator);
    }

    pub fn horizontal_separator(&mut self) {
        self.push(UiCommand::HorizontalSeparator);
    }

    pub fn vertical_separator(&mut self) {
        self.push(UiCommand::VerticalSeparator);
    }

    pub fn space(&mut self, pixels: f32) {
        self.push(UiCommand::Space(pixels));
    }

    pub fn radio_param(&mut self, index: usize, label: String, choices: Vec<String>) {
        self.push(UiCommand::RadioParam {
            index,
            label,
            choices,
        });
    }

    pub fn button(&mut self, id: String, label: String) {
        let clicked = self.clicked.iter().any(|clicked| clicked == &id);
        self.push(UiCommand::Button { id, label, clicked });
    }

    pub fn param(&mut self, index: usize, label: String, scale: Option<MacroScale>) {
        self.push(UiCommand::Param {
            index,
            label,
            scale,
        });
    }

    pub fn meter(&mut self, value: f32, label: Option<String>) {
        self.push(UiCommand::Meter { value, label });
    }

    pub fn begin_row(&mut self) {
        self.stack.push(Frame::Row(Vec::new()));
    }

    pub fn end_row(&mut self) -> Result<(), &'static str> {
        self.end_scope(true)
    }

    pub fn begin_column(&mut self) {
        self.stack.push(Frame::Column(Vec::new()));
    }

    pub fn end_column(&mut self) -> Result<(), &'static str> {
        self.end_scope(false)
    }

    fn end_scope(&mut self, is_row: bool) -> Result<(), &'static str> {
        let Some(frame) = self.stack.pop() else {
            return Err("unbalanced ui.row()/ui.column() calls");
        };

        match (frame, is_row) {
            (Frame::Row(children), true) => {
                self.push(UiCommand::Row(children));
                Ok(())
            }
            (Frame::Column(children), false) => {
                self.push(UiCommand::Column(children));
                Ok(())
            }
            (frame, _) => {
                self.stack.push(frame);
                Err("mismatched ui.row()/ui.column() calls")
            }
        }
    }

    fn push(&mut self, command: UiCommand) {
        if let Some(Frame::Root(commands) | Frame::Row(commands) | Frame::Column(commands)) =
            self.stack.last_mut()
        {
            commands.push(command);
        }
    }
}
