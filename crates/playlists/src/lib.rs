//! 歌单：手动歌单（按路径记下曲目与顺序）与智能歌单（按元数据、标签、音频参数、
//! 文件属性的条件从曲库里自动聚合）。持久化到数据目录的 `playlists.json`。

mod fields;
mod model;
mod rule;
mod store;
#[cfg(test)]
mod tests;

pub use fields::*;
pub use model::*;
pub use rule::*;
pub use store::*;
