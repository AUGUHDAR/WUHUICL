using System.Windows;
using System.Windows.Controls;

namespace Wuhuicl.App.Pages;

/// <summary>
/// MOD 面板（资源管理子项，对应 BridgeView.vue）：运行中实例的 MOD 参数实时编辑器，与联机无关。
///
/// 结构（BridgeView.vue:186-286）：页头（:187-190，无右侧按钮）→ 状态卡 .card.bridge-status
/// （:192-206，:332 重排为 24px + minmax(0,1fr) 两列 grid）→ 参数区（:208-284）。
/// 没有页签、没有模态、没有子模式，所以本页也不需要任何模式切换代码。
///
/// 契约 §5 的 LaunchSession 白名单里没有桥接状态、参数清单、modId、实例关联字段，
/// 因此整页零 Binding，参数行只是把原版的「行语法」用写死文案铺出来（// 占位：等桥接 MOD 功能接入）。
/// status?.connected（:208）为假时参数区不渲染，所以 ParamsSection 默认 Collapsed；
/// 状态文案取 :196 六态里的「先选择游戏实例」一组，其余五态在 XAML 注释里逐条留档。
///
/// 事件都留在本页（契约 §1），不引用 MainWindow 的任何方法；本轮唯一的行为是搜索框 placeholder 覆盖层。
/// </summary>
public partial class BridgePage : UserControl
{
    public BridgePage()
    {
        InitializeComponent();
    }

    /// <summary>:210 的 input placeholder「搜索参数名称、说明、分组…」，WPF 的 TextBox 没有 placeholder，
    /// 按 ServersPage 的覆盖文字写法：空内容时才显示占位行。</summary>
    void Search_TextChanged(object sender, TextChangedEventArgs e)
        => SearchPh.Visibility = TxtSearch.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;

    // 供主控接线时定位：
    // BridgeDot —— :294 的 .bridge-dot.on 呼吸脉冲（:295-298 关键帧）挂点，接色时用 ok 而不是这里的 Fg2；
    // ParamsSection —— :208 的 v-if="status?.connected" 分支壳，与 TxtSearch（:210 搜索框）互斥于两条空态
    //   （:281 没有匹配「…」的参数 / :282 桥接 MOD 没有注册任何参数）；
    // BtnResetAll —— :204 的「全部恢复默认」，:201 的「安装桥接 MOD」与 :203 的「前往首页 / 选择游戏实例」
    //   同处一块，四条的 v-if 条件都标在 XAML 行内注释里。
}
