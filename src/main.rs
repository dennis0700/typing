mod app_state;
mod audio;
mod domain;
mod storage;
// 开发期调试追踪设施（`TYPING_TRACE=1` 启用）。**临时诊断代码，非产品功能**，
// 渲染问题定位完成后应连同 `ui.rs` 中的埋点一并移除。
// 必须声明在 `mod ui;` 之前：`trace!` 宏虽然是 `#[macro_export]`（可用
// `crate::trace!` 全路径调用），但把定义放在使用者之前更符合阅读顺序。
mod trace;
mod ui;

/// 应用入口：委托给 `ui::run_app()` 完成 `AppController` 初始化（含
/// `ProfileStore` 默认路径解析）、Slint 主窗口创建、全部回调注册与事件循环
/// 启动（任务 18.8 已实现，参见 `ui.rs` 模块文档）。
///
/// 启动时档案选择界面/课程序列界面的路由分支：`app.slint` 中
/// `current-page` 的默认值即为 `AppPage.profile-select`，`run_app()` 在
/// 创建窗口后不会基于已有档案数量修改该初始值——这与 Req 6.2（"设备上
/// 存在 1 个或以上学员档案时，应用 SHALL 在启动时提供学员档案选择界面，
/// 列出所有已创建的学员档案及其昵称"）的措辞一致：无论是否已有档案，
/// 启动时都应展示档案选择界面（列出已有档案供选择，而非跳过该界面自动
/// 进入课程序列），因此这里不需要额外实现"已有档案时自动路由到课程序列
/// 界面"的逻辑。
///
/// `AppController::new_with_default_store` 在无法解析用户主目录时返回
/// `Err`；这里不 panic/unwrap，而是将错误信息打印到 stderr 并以非零状态
/// 退出，交由调用方（操作系统/父进程）感知启动失败。
fn main() {
    if let Err(err) = ui::run_app() {
        eprintln!("应用启动失败：{err}");
        std::process::exit(1);
    }
}
