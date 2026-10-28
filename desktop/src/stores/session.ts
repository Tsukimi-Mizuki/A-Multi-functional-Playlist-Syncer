import { computed, ref } from "vue";
import { defineStore } from "pinia";
import * as api from "@/lib/api";
import { errorMessageOf } from "@/lib/toast";
import type { SessionDto, UserDto } from "@/lib/types";

export const useSessionStore = defineStore("session", () => {
  const session = ref<SessionDto>({ loggedIn: false, user: null });
  const loading = ref(false);
  const bootstrapped = ref(false);
  const error = ref<string | null>(null);

  const user = computed<UserDto | null>(() => session.value.user);
  const loggedIn = computed(() => session.value.loggedIn && !!session.value.user);
  const nickname = computed(() => (user.value ? user.value.nickname : "未登录"));
  const avatarUrl = computed(() => (user.value ? user.value.avatarUrl : null));
  const vipType = computed(() => (user.value ? user.value.vipType : 0));
  const vipLabel = computed(() => {
    if (!user.value) return "";
    if (vipType.value <= 0) return "普通用户";
    return vipType.value >= 10 ? "黑胶 VIP" : "音乐包";
  });

  async function refresh(): Promise<boolean> {
    loading.value = true;
    error.value = null;
    try {
      session.value = await api.getSession();
      return !!session.value.loggedIn;
    } catch (cause) {
      error.value = errorMessageOf(cause, "读取登录状态失败");
      session.value = { loggedIn: false, user: null };
      return false;
    } finally {
      loading.value = false;
      bootstrapped.value = true;
    }
  }

  function applyUser(nextUser: UserDto): void {
    session.value = { loggedIn: true, user: nextUser };
    bootstrapped.value = true;
    error.value = null;
  }

  async function signOut(): Promise<void> {
    try {
      await api.logout();
    } catch {
      /* 后端未启动时也允许清理本地状态 */
    }
    session.value = { loggedIn: false, user: null };
  }

  return {
    session,
    loading,
    bootstrapped,
    error,
    user,
    loggedIn,
    nickname,
    avatarUrl,
    vipType,
    vipLabel,
    refresh,
    applyUser,
    signOut,
  };
});
