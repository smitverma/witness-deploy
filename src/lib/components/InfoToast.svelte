<script lang="ts">
  import Toast from "./Toast.svelte";
  import { dismissInfoToast, infoToasts } from "$lib/infoToast";
</script>

<!-- Offset above error toasts so queued error + info toasts never overlap. -->
<Toast toasts={$infoToasts} tone="info" bottomPx={118} onDismiss={(id) => dismissInfoToast(id)} />
