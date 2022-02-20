use crate::adapted_iter::SingleAdaptedFileAsIter;

use super::*;
use anyhow::*;
use log::*;

use crate::adapters::FileAdapter;
use std::process::Command;
use std::process::{Child, Stdio};
use std::{io::prelude::*, path::Path};

// TODO: don't separate the trait and the struct
pub trait SpawningFileAdapterTrait: GetMetadata {
    fn get_exe(&self) -> &str;
    fn command(&self, filepath_hint: &Path, command: Command) -> Result<Command>;
}

pub struct SpawningFileAdapter {
    inner: Box<dyn SpawningFileAdapterTrait>,
}

impl SpawningFileAdapter {
    pub fn new(inner: Box<dyn SpawningFileAdapterTrait>) -> SpawningFileAdapter {
        SpawningFileAdapter { inner }
    }
}

impl GetMetadata for SpawningFileAdapter {
    fn metadata(&self) -> &AdapterMeta {
        self.inner.metadata()
    }
}

/*impl<T: SpawningFileAdapterTrait> From<T> for SpawningFileAdapter {
    fn from(e: dyn T) -> Self {
        SpawningFileAdapter { inner: Box::new(e) }
    }
}*/

/// replace a Command.spawn() error "File not found" with a more readable error
/// to indicate some program is not installed
pub fn map_exe_error(err: std::io::Error, exe_name: &str, help: &str) -> Error {
    use std::io::ErrorKind::*;
    match err.kind() {
        NotFound => format_err!("Could not find executable \"{}\". {}", exe_name, help),
        _ => Error::from(err),
    }
}

/** waits for a process to finish, returns an io error if the process failed */
struct ProcWaitReader {
    proce: Child,
}
impl Read for ProcWaitReader {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        let status = self.proce.wait()?;
        if status.success() {
            Ok(0)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format_err!("subprocess failed: {:?}", status),
            ))
        }
    }
}
pub fn pipe_output<'a>(
    _line_prefix: &str,
    mut cmd: Command,
    inp: &mut (dyn Read + 'a),
    exe_name: &str,
    help: &str,
) -> Result<ReadBox<'a>> {
    let mut cmd = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| map_exe_error(e, exe_name, help))?;
    let mut stdi = cmd.stdin.take().expect("is piped");
    let stdo = cmd.stdout.take().expect("is piped");

    // TODO: deadlocks since this is run in the same thread as the thing reading from stdout of the process
    std::io::copy(inp, &mut stdi)?;
    drop(stdi);

    Ok(Box::new(stdo.chain(ProcWaitReader { proce: cmd })))
}

impl FileAdapter for SpawningFileAdapter {
    fn adapt<'a>(
        &self,
        ai: AdaptInfo<'a>,
        _detection_reason: &FileMatcher,
    ) -> Result<AdaptedFilesIterBox<'a>> {
        let AdaptInfo {
            filepath_hint,
            mut inp,
            line_prefix,
            archive_recursion_depth,
            postprocess,
            config,
            is_real_file,
        } = ai;

        let cmd = Command::new(self.inner.get_exe());
        let cmd = self
            .inner
            .command(&filepath_hint, cmd)
            .with_context(|| format!("Could not set cmd arguments for {}", self.inner.get_exe()))?;
        debug!("executing {:?}", cmd);
        let output = pipe_output(&line_prefix, cmd, &mut inp, self.inner.get_exe(), "")?;
        Ok(Box::new(SingleAdaptedFileAsIter::new(AdaptInfo {
            filepath_hint: PathBuf::from(format!("{}.txt", filepath_hint.to_string_lossy())), // TODO: customizable
            inp: output,
            line_prefix,
            is_real_file: false,
            archive_recursion_depth,
            postprocess,
            config,
        })))
    }
}

#[cfg(test)]
mod test {
    use std::io::Cursor;

    use super::*;
    use crate::adapters::FileAdapter;
    use crate::{
        adapters::custom::CustomAdapterConfig,
        test_utils::{adapted_to_vec, simple_adapt_info},
    };

    #[test]
    fn streaming() {
        // an adapter that converts input line by line (deadlocks if the parent process tries to write everything and only then read it)
        let adapter = CustomAdapterConfig {
            name: "simple text replacer".to_string(),
            description: "oo".to_string(),
            disabled_by_default: None,
            version: 1,
            extensions: vec!["txt".to_string()],
            mimetypes: None,
            match_only_by_mime: None,
            binary: "sed".to_string(),
            args: vec!["s/e/u/g".to_string()],
        };

        let adapter = adapter.to_adapter();
        let input = r#"
        This is the story of a
        very strange lorry
        with a long dead crew
        and a witch with the flu
        "#;
        let input = format!("{0}{0}{0}{0}", input);
        let input = format!("{0}{0}{0}{0}", input);
        let input = format!("{0}{0}{0}{0}", input);
        let input = format!("{0}{0}{0}{0}", input);
        let input = format!("{0}{0}{0}{0}", input);
        let input = format!("{0}{0}{0}{0}", input);
        let (a, d) = simple_adapt_info(
            &Path::new("foo.txt"),
            Box::new(Cursor::new(input.as_bytes())),
        );
        let output = adapter.adapt(a, &d).unwrap();

        let oup = adapted_to_vec(output).unwrap();
        println!("output: {}", String::from_utf8_lossy(&oup));
    }
}
