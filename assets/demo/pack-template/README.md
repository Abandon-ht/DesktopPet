# 可分发角色包配置模板

本目录只包含项目自写的配置示例，不包含第三方角色、贴图或模型。

将 `manifest.example.json` 复制到自己的独立角色包目录并改名为 `manifest.json`，添加自己有权使用的模型文件，再按 `schemas/avatar-pack-v1.schema.json` 配置。模板缺少模型，不能直接导入运行。

需要数值驱动基础表情、分段互动和逐项预览时，可改用 `manifest.v2.example.json` 及 `schemas/avatar-pack-v2.schema.json`。v2 的 `expression_profile.catalog` 先登记包内表情文件，再由 `baseline` 和 `reactions` 引用目录 ID；至少配置 `neutral` 基础表情。模板中的表情路径只是占位，需替换为自己有权使用的资源。v1 包继续可用，但不具备完整表情编排。

需要细分点击区域时，使用 `manifest.v3.example.json` 与 `schemas/avatar-pack-v3.schema.json`。v3 保留 v2 表情编排，在 `interaction.regions` 中添加脸、画面左／右手、上／下半身的可选凸多边形；至少配置一个细分区域。细分区域优先于 `head` 和 `body`，后两者仍作为回退。示例坐标仅用于说明格式，必须依角色画面重新标定。

需要校准更多可见区域时，可使用 `manifest.v4.example.json` 与 `schemas/avatar-pack-v4.schema.json`。v4 可再定义画面左／右手臂、腹部、左／右腿与左／右脚；这些细分区先于宽泛身体区判定。若同一区域需要覆盖分开的形状，可在可选的 `interaction.region_parts` 中为已有 `regions` 区域补充 1–8 个凸多边形；示例给下半身增加左右两片。模板坐标只是示意，未校准的角色包不应据此修改养成数值。

v4 还可选配 `touch_reactions`，将程序返回的语义提示（如 `head_warm`、`uneasy`、`boundary_first`）绑定为 1–3 段表情，段内 `expression` 必须指向 `expression_profile.catalog` 中的 ID。需要同一部位在不同表情间变化时，可用 `touch_variants` 为该区域列出 2–4 个已经登记在 `touch_reactions` 的提示；每次有效点击只伪随机播放其中一个。角色包没有这些配置时沿用旧点击反馈。测试真实点击前先校准区域；限制区的数值规则由程序统一计算，角色包只提供表情资源。

坐标是顶部为原点、5:6 宠物视口的归一化值；头与身体使用严格凸多边形，头区域优先。锚点和区域必须按自己的角色校准，不是 Live2D Drawable ID。八项动作均可为 null；新增 `feed`、`play`、`rest`、`greet`、`peek`、`invite` 为 P2 表情。未配置时会回退到可用的头部或身体点击表情，若两者也没有，则只更新养成状态。每项动作使用本地 `.exp3.json` 路径与 100–5000 毫秒时长。

`interaction.anchor` 仍用于屏幕底部的脚底位置；可选的 `interaction.window_perch_y` 控制其他应用窗口的上沿穿过角色的高度，范围 0.2–0.8，省略时为 0.5。用户可以在设置面板为每个角色微调该高度；微调保存在本机设置中，不修改角色包。
