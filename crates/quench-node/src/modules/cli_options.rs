//! Parsed startup values for implemented Node feature switches.
const BOOLEAN_OPTIONS: &[(&str, bool)] = &[
    ("--experimental-vfs", false),
    ("--experimental-stream-iter", false),
    ("--expose-internals", false),
    ("--warnings", true),
    ("--report-compact", false),
    ("--report-exclude-env", false),
    ("--report-exclude-network", false),
    ("--report-on-fatalerror", false),
    ("--report-on-signal", false),
    ("--report-uncaught-exception", false),
];
const STRING_OPTIONS: &[(&str, &str)] = &[
    ("--report-dir", ""),
    ("--report-filename", ""),
    ("--report-signal", "SIGUSR2"),
];
fn canonical_option(option: &str) -> &str { if option == "--report-directory" { "--report-dir" } else { option } }
pub(crate) fn takes_string_value(option: &str) -> bool { STRING_OPTIONS.iter().any(|(name, _)| *name == canonical_option(option)) }
#[derive(Clone, Debug)]
pub(crate) struct CliOptions {
    booleans: std::collections::BTreeMap<&'static str, bool>,
    strings: std::collections::BTreeMap<&'static str, String>,
}
impl Default for CliOptions {
    fn default() -> Self { Self {
        booleans: BOOLEAN_OPTIONS.iter().copied().collect(),
        strings: STRING_OPTIONS.iter().map(|(name, value)| (*name, (*value).to_owned())).collect(),
    } }
}
impl CliOptions {
    pub(crate) fn parse(environment: Option<&str>, invocation: &[String]) -> Result<Self, String> {
        let mut options=Self::default();
        if let Some(environment)=environment { options.apply(&parse_node_options(environment).map_err(str::to_owned)?)?; }
        options.apply(invocation)?;Ok(options)
    }
    pub(crate) fn enabled(&self,name: &str) -> bool { self.booleans.get(name).copied().unwrap_or(false) }
    pub(crate) fn values(&self) -> impl Iterator<Item=(&'static str,bool)> + '_ { self.booleans.iter().map(|(name,value)|(*name,*value)) }
    pub(crate) fn string_value(&self, name: &str) -> Option<&str> { self.strings.get(canonical_option(name)).map(String::as_str) }
    pub(crate) fn strings(&self) -> impl Iterator<Item=(&'static str,&str)> + '_ { self.strings.iter().map(|(name,value)|(*name,value.as_str())) }
    fn apply(&mut self, arguments: &[String]) -> Result<(), String> {
        let mut arguments=arguments.iter().peekable();
        while let Some(argument)=arguments.next() {
            let flag=canonical_option(argument.split_once('=').map_or(argument.as_str(),|(flag,_)|flag));
            if let Some(value) = self.strings.get_mut(flag) {
                *value = argument.split_once('=').map(|(_, value)| value.to_owned())
                    .or_else(|| arguments.next().cloned()).ok_or_else(|| format!("{flag} requires an argument"))?;
                if flag == "--report-signal" { self.booleans.insert("--report-on-signal", true); }
                continue;
            }
            match flag {
                "--trace-event-categories"|"--trace-event-file-pattern"|"-r"|"--require" if !argument.contains('=') => { arguments.next(); }
                "-e"|"--eval"|"-pe"|"-ep" if !argument.contains('=') => { arguments.next(); }
                "-p"|"--print" if !argument.contains('=') && arguments.peek().is_some_and(|next|!next.starts_with('-')) => { arguments.next(); }
                _ => {
                    if let Some(value)=self.booleans.get_mut(flag) { *value=true; }
                    else if let Some(positive)=flag.strip_prefix("--no-") {
                        let positive=format!("--{positive}");
                        if let Some(value)=self.booleans.get_mut(positive.as_str()) { *value=false; }
                    }
                }
            }
        }
        Ok(())
    }
}

/// Node's double-quote tokenizer; only unquoted ASCII spaces separate arguments.
pub(crate) fn parse_node_options(text: &str) -> Result<Vec<String>, &'static str> {
    let mut arguments=Vec::<String>::new();let mut quoted=false;let mut new_argument=true;
    let mut characters=text.chars();
    while let Some(mut character)=characters.next() {
        if character=='\\' && quoted {
            character=characters.next().ok_or("invalid value for NODE_OPTIONS (invalid escape)\n")?;
        } else if character==' ' && !quoted { new_argument=true;continue; }
        else if character=='"' { quoted=!quoted;continue; }
        if new_argument { arguments.push(String::new());new_argument=false; }
        arguments.last_mut().expect("argument created before character").push(character);
    }
    if quoted { return Err("invalid value for NODE_OPTIONS (unterminated string)\n"); }
    Ok(arguments)
}
