<script lang="ts">
  import Toast from "./Toast.svelte";
  import { dismissErrorToast, errorToasts } from "$lib/errorToast";
</script>

<Toast toasts={$errorToasts} tone="error" bottomPx={46} onDismiss={(id) => dismissErrorToast(id)} />
