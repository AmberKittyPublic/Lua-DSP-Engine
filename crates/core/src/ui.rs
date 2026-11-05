use crate::params::MacroScale;

#[derive(Debug, Clone)]
pub enum UiCommand {
    Label(String),
    Heading {
        text: String,
        color: Option<String>,
    },
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
    Knob {
        index: usize,
        label: String,
        scale: Option<MacroScale>,
    },
    Toggle {
        index: usize,
        label: String,
    },
    Meter {
        value: f32,
        label: Option<String>,
        color: Option<String>,
    },
    Waveform,
    VerticalMeter { value: f32 },
    LevelGraph,
    Graph {
        children: Vec<UiCommand>,
    },
    Plot {
        values: Vec<f32>,
        color: Option<String>,
    },
        CompressionCurve {
        threshold: f32,
        ratio: f32,
        knee: f32,
    },
    CompressorGraph {
        threshold: f32,
        ratio: f32,
        knee: f32,
    },
    Curve {
        drive: f32,
        bias: f32,
        shape: f32,
        output: f32,
    },    Row(Vec<UiCommand>),
    Column(Vec<UiCommand>),
    Group {
        title: Option<String>,
        children: Vec<UiCommand>,
    },
    Collapsing {
        title: String,
        children: Vec<UiCommand>,
    },
}

enum Frame {
    Root(Vec<UiCommand>),
    Row(Vec<UiCommand>),
    Column(Vec<UiCommand>),
    Group {
        title: Option<String>,
        children: Vec<UiCommand>,
    },
    Collapsing {
        title: String,
        children: Vec<UiCommand>,
    },
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
        self.push(UiCommand::Heading { text, color: None });
    }

    pub fn colored_heading(&mut self, text: String, color: String) {
        self.push(UiCommand::Heading {
            text,
            color: Some(color),
        });
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

    pub fn knob(&mut self, index: usize, label: String, scale: Option<MacroScale>) {
        self.push(UiCommand::Knob {
            index,
            label,
            scale,
        });
    }

    pub fn toggle(&mut self, index: usize, label: String) {
        self.push(UiCommand::Toggle { index, label });
    }

    pub fn meter(&mut self, value: f32, label: Option<String>) {
        self.push(UiCommand::Meter {
            value,
            label,
            color: None,
        });
    }

    pub fn colored_meter(&mut self, value: f32, label: Option<String>, color: String) {
        self.push(UiCommand::Meter {
            value,
            label,
            color: Some(color),
        });
    }

    pub fn waveform(&mut self) {
        self.push(UiCommand::Waveform);
    }

    pub fn vertical_meter(&mut self, value: f32) {
        self.push(UiCommand::VerticalMeter { value });
    }

    pub fn begin_graph(&mut self) {
        self.stack.push(Frame::Group {
            title: None,
            children: Vec::new(),
        });
        // We need a distinct frame type; reuse Group but mark? Simpler: treat as Group with no title.
    }

    pub fn end_graph(&mut self) -> Result<(), &'static str> {
        let Some(frame) = self.stack.pop() else {
            return Err("unbalanced ui.graph() calls");
        };
        match frame {
            Frame::Group { title: _, children } => {
                self.push(UiCommand::Graph { children });
                Ok(())
            }
            frame => {
                self.stack.push(frame);
                Err("mismatched ui.graph() calls")
            }
        }
    }

    pub fn plot(&mut self, values: Vec<f32>, color: Option<String>) {
        self.push(UiCommand::Plot { values, color });
    }    pub fn level_graph(&mut self) {
        self.push(UiCommand::LevelGraph);
    }

        pub fn compression_curve(&mut self, threshold: f32, ratio: f32, knee: f32) {
        self.push(UiCommand::CompressionCurve {
            threshold,
            ratio,
            knee,
        });
    }

    pub fn compressor_graph(&mut self, threshold: f32, ratio: f32, knee: f32) {
        self.push(UiCommand::CompressorGraph {
            threshold,
            ratio,
            knee,
        });
    }    pub fn curve(&mut self, drive: f32, bias: f32, shape: f32, output: f32) {
        self.push(UiCommand::Curve {
            drive,
            bias,
            shape,
            output,
        });
    }
    pub fn begin_group(&mut self, title: Option<String>) {
        self.stack.push(Frame::Group {
            title,
            children: Vec::new(),
        });
    }

    pub fn end_group(&mut self) -> Result<(), &'static str> {
        self.end_group_scope()
    }

    pub fn begin_collapsing(&mut self, title: String) {
        self.stack.push(Frame::Collapsing {
            title,
            children: Vec::new(),
        });
    }

    pub fn end_collapsing(&mut self) -> Result<(), &'static str> {
        self.end_collapsing_scope()
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

    fn end_group_scope(&mut self) -> Result<(), &'static str> {
        let Some(frame) = self.stack.pop() else {
            return Err("unbalanced ui.group() calls");
        };

        match frame {
            Frame::Group { title, children } => {
                self.push(UiCommand::Group { title, children });
                Ok(())
            }
            frame => {
                self.stack.push(frame);
                Err("mismatched ui.group() calls")
            }
        }
    }

    fn end_collapsing_scope(&mut self) -> Result<(), &'static str> {
        let Some(frame) = self.stack.pop() else {
            return Err("unbalanced ui.collapsing() calls");
        };

        match frame {
            Frame::Collapsing { title, children } => {
                self.push(UiCommand::Collapsing { title, children });
                Ok(())
            }
            frame => {
                self.stack.push(frame);
                Err("mismatched ui.collapsing() calls")
            }
        }
    }

    fn push(&mut self, command: UiCommand) {
        if let Some(
            Frame::Root(commands)
            | Frame::Row(commands)
            | Frame::Column(commands)
            | Frame::Group {
                children: commands, ..
            }
            | Frame::Collapsing {
                children: commands, ..
            },
        ) = self.stack.last_mut()
        {
            commands.push(command);
        }
    }
}
