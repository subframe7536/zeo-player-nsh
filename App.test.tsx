// `nsh test`：无头运行 Rust 根组件，检查导航与设置页。
import { expect, open, test } from "nsh/test";

test("sidebar navigates between library views", async () => {
  const app = await open(".");
  await app.waitFor("文件夹");
  expect(app.texts()).toContain("歌手");
  expect(app.texts()).toContain("专辑");
  expect(app.texts()).toContain("标签");
  await app.press("专辑");
  expect(app.texts()).toContain("未在播放");
  await app.press("设置");
  await app.waitFor("音乐文件夹");
  expect(app.texts()).toContain("曲库");
  expect(app.texts()).toContain("添加文件夹…");
  expect(app.texts()).toContain("重建曲库");
  expect(app.texts()).toContain("流派标签");
  expect(app.texts()).toContain("分号");
  expect(app.texts()).toContain("监视文件夹变化");
  expect(app.texts()).toContain("系统字体");
});

test("folders page lists the root folders with a size column", async () => {
  const app = await open(".", { width: 1200, height: 800 });
  await app.waitFor("文件夹");
  if (app.texts().includes("还没有音乐文件夹")) return; // 没有配置根目录
  await app.waitFor("大小");
  const texts = app.texts();
  expect(texts.some((t) => t.includes("个根文件夹"))).toBe(true);
  expect(texts).toContain("时长");
});

test("narrow windows drop the album and size columns", async () => {
  const app = await open(".", { width: 1200, height: 800 });
  await app.waitFor("文件夹");
  if (app.texts().includes("还没有音乐文件夹")) return;
  await app.waitFor("大小");
  await app.setViewport(700, 800); // 内容区 468 pt
  expect(app.texts()).not.toContain("大小");
  // 侧栏也有「专辑」，列头的那个应已收起。
  expect(app.texts().filter((t) => t === "专辑").length).toBe(1);
  expect(app.texts()).toContain("时长");
});

test("full player opens and closes", async () => {
  const app = await open(".", { width: 1200, height: 800 });
  await app.waitFor("文件夹");
  await app.press("label:全屏播放");
  await app.waitFor("label:收起");
  expect(app.texts().some((t) => t === "暂无歌词" || t === "正在载入歌词…" || t === "Zeo")).toBe(true);
  await app.press("label:收起");
});

test("font list loads on demand and is gone after leaving settings", async () => {
  const app = await open(".", { width: 1200, height: 800 });
  await app.press("设置");
  await app.waitFor("更改字体…");
  expect(app.texts()).not.toContain("正在读取字体…");
  await app.press("更改字体…");
  await app.waitFor("label:收起字体列表");
  await app.waitUntil(() => app.texts().some((t) => /^\d+ 个$/.test(t)));
  await app.press("文件夹");
  await app.press("设置");
  await app.waitFor("更改字体…"); // 回来时列表已销毁，重新收起
  expect(app.texts().some((t) => /^\d+ 个$/.test(t))).toBe(false);
});
