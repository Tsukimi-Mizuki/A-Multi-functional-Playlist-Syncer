<script setup lang="ts">
const props = withDefaults(defineProps<{ colspan?: number; text?: string; class?: string }>(), {
  colspan: 8,
  text: "暂无数据",
});
</script>

<template>
  <tr>
    <td :colspan="props.colspan" class="px-4 py-10 text-center text-xs text-muted-foreground">
      <slot>{{ props.text }}</slot>
    </td>
  </tr>
</template>
