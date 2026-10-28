import { createRouter, createWebHashHistory } from "vue-router";
import MainPage from "@/pages/MainPage.vue";

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: "/",
      name: "main",
      component: MainPage,
      meta: { title: "曲库同步" },
    },
    {
      path: "/login",
      name: "login",
      component: () => import("@/pages/LoginPage.vue"),
      meta: { title: "扫码登录" },
    },
    {
      path: "/ncm",
      name: "ncm",
      component: () => import("@/pages/NcmImportPage.vue"),
      meta: { title: "导入并解密 NCM" },
    },
    {
      path: "/lyrics",
      name: "lyrics",
      component: () => import("@/pages/LyricsEditorPage.vue"),
      meta: { title: "歌词与标签编辑" },
    },
    {
      path: "/batch-lyrics",
      name: "batch-lyrics",
      component: () => import("@/pages/BatchLyricsPage.vue"),
      meta: { title: "批量获取标签、封面与歌词" },
    },
    { path: "/:pathMatch(.*)*", redirect: "/" },
  ],
});

export default router;
