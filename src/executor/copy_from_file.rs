// Copyright 2022 RisingLight Project Authors. Licensed under Apache-2.0.

use std::fs::File;
use std::io::BufReader;

use indicatif::{ProgressBar, ProgressStyle};
use tokio::sync::mpsc::Sender;

use super::*;
use crate::array::ArrayBuilderImpl;
use crate::binder::FileFormat;
use crate::optimizer::plan_nodes::PhysicalCopyFromFile;

/// The executor of loading file data.
pub struct CopyFromFileExecutor {
    pub context: Arc<Context>,
    pub plan: PhysicalCopyFromFile,
}

/// When the source file size is above the limit, we show a progress bar on the screen.
const IMPORT_PROGRESS_BAR_LIMIT: u64 = 1024 * 1024;

impl CopyFromFileExecutor {
    #[try_stream(boxed, ok = DataChunk, error = ExecutorError)]
    pub async fn execute(self) {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let context = self.context.clone();
        match context.spawn_blocking(|token| self.read_file_blocking(tx, token)) {
            Some(handle) => {
                while let Some(chunk) = rx.recv().await {
                    yield chunk;
                }
                handle.await.unwrap()?;
            }
            None => return Err(ExecutorError::Abort),
        }
    }

    /// Read records from file using blocking IO.
    ///
    /// The read data chunks will be sent through `tx`.
    fn read_file_blocking(
        self,
        tx: Sender<DataChunk>,
        token: CancellationToken,
    ) -> Result<(), ExecutorError> {
        let file = File::open(&self.plan.logical().path())?;
        let file_size = file.metadata()?.len();
        let mut buf_reader = BufReader::new(file);
        let mut reader = match self.plan.logical().format().clone() {
            FileFormat::Csv {
                delimiter,
                quote,
                escape,
                header,
            } => csv::ReaderBuilder::new()
                .delimiter(delimiter as u8)
                .quote(quote as u8)
                .escape(escape.map(|c| c as u8))
                .has_headers(header)
                .from_reader(&mut buf_reader),
        };

        let bar = if file_size < IMPORT_PROGRESS_BAR_LIMIT {
            // disable progress bar if file size is < 1MB
            ProgressBar::hidden()
        } else {
            let bar = ProgressBar::new(file_size);
            bar.set_style(
                ProgressStyle::default_bar()
                    .template("[{elapsed_precise}] {bar:40.cyan/blue} {bytes}/{total_bytes}")
                    .progress_chars("=>-"),
            );
            bar
        };

        let column_count = self.plan.logical().column_types().len();
        let mut iter = reader.records();
        let mut finished = false;
        while !finished {
            // create array builders
            let mut array_builders = self
                .plan
                .logical()
                .column_types()
                .iter()
                .map(|ty| ArrayBuilderImpl::with_capacity(PROCESSING_WINDOW_SIZE, ty))
                .collect_vec();

            // read records and push to array builder
            for _ in 0..PROCESSING_WINDOW_SIZE {
                let record = match iter.next() {
                    Some(record) => record?,
                    None => {
                        finished = true;
                        break;
                    }
                };
                if !(record.len() == column_count
                    || record.len() == column_count + 1 && record.get(column_count) == Some(""))
                {
                    return Err(ExecutorError::LengthMismatch {
                        expected: column_count,
                        actual: record.len(),
                    });
                }
                for ((s, builder), ty) in record
                    .iter()
                    .zip(&mut array_builders)
                    .zip(&self.plan.logical().column_types().to_vec())
                {
                    if !ty.is_nullable() && s.is_empty() {
                        return Err(ExecutorError::NotNullable);
                    }
                    builder.push_str(s)?;
                }
            }
            // update progress bar
            bar.set_position(iter.reader().position().byte());

            // send data chunk
            let chunk: DataChunk = array_builders.into_iter().collect();

            #[allow(clippy::collapsible_if)]
            if chunk.cardinality() > 0 {
                if token.is_cancelled() || tx.blocking_send(chunk).is_err() {
                    return Err(ExecutorError::Abort);
                }
            }
        }
        bar.finish();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::array::ArrayImpl;
    use crate::types::{DataTypeExt, DataTypeKind};

    #[tokio::test]
    async fn read_csv() {
        let csv = "1,1.5,one\n2,2.5,two\n";

        let mut file = tempfile::NamedTempFile::new().expect("failed to create temp file");
        write!(file, "{}", csv).expect("failed to write file");

        let executor = CopyFromFileExecutor {
            context: Default::default(),
            plan: PhysicalCopyFromFile::new(LogicalCopyFromFile::new(
                file.path().into(),
                FileFormat::Csv {
                    delimiter: ',',
                    quote: '"',
                    escape: None,
                    header: false,
                },
                vec![
                    DataTypeKind::Int(None).not_null(),
                    DataTypeKind::Double.not_null(),
                    DataTypeKind::String.not_null(),
                ],
                vec![
                    DataTypeKind::Int(None).not_null().to_column("v1".into()),
                    DataTypeKind::Double.not_null().to_column("v2".into()),
                    DataTypeKind::String.not_null().to_column("v3".into()),
                ],
            )),
        };
        let actual = executor.execute().next().await.unwrap().unwrap();

        let expected: DataChunk = [
            ArrayImpl::Int32([1, 2].into_iter().collect()),
            ArrayImpl::Float64([1.5, 2.5].into_iter().collect()),
            ArrayImpl::Utf8(["one", "two"].iter().map(Some).collect()),
        ]
        .into_iter()
        .collect();
        assert_eq!(actual, expected);
    }
}
