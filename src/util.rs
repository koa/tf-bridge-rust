use std::future::Future;

use tokio_stream::{empty, Empty, Stream};
use tokio_util::either::Either;

pub async fn optional_stream<ISF: Future<Output = IS>, IS: Stream<Item = T>, T>(
    is: Option<ISF>,
) -> Either<IS, Empty<T>> {
    if let Some(is) = is {
        Either::Left(is.await)
    } else {
        Either::Right(empty::<T>())
    }
}
