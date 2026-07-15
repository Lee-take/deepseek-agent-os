import type {
  AccessMode,
  AgentRunStatus,
  CapabilityAccessStatus,
  CapabilityFamily,
  CapabilityGrantState,
  CapabilityInvocationStatus,
  CapabilityKind,
  CodexBridgeTransport,
  ComputerControlBackend,
  ComputerScreenshotBackend,
  DriveBackend,
  EmailBackend,
  LargeModelProvider,
  Language,
  MemoryLifecycle,
  MemoryMaintenanceActionKind,
  MemoryMaintenanceReviewKind,
  MemoryRelationKind,
  MemorySearchMatchSource,
  MemorySelectedFeedbackKind,
  MemoryScope,
  MemorySensitivity,
  MemoryCandidateSuggestedAction,
  MemoryCandidateStatus,
  MemoryType,
  ModelRoute,
  ToolExecutionStatus,
  NetworkSearchBackend,
  NetworkSearchEvidencePolicy,
  NetworkSearchExecutionMode,
  NetworkSearchSourceModel,
  OperationsBriefingRunStatus,
  PolicyDecision,
  RiskLevel,
  RuntimePlatform,
  TerminalReadCommand,
  ThemeStyle,
  ThinkingLevel,
  WorkspaceScope,
} from "./types";

type RunStatusStepState = "done" | "current" | "waiting" | "needs_action" | "blocked";

type TranslationSet = {
  brandTagline: string;
  navLabel: string;
  nav: {
    workbench: string;
    memory: string;
    approvals: string;
    newChat: string;
    conversations: string;
    untitledConversation: string;
    plugins: string;
    settings: string;
    contextCompressed: string;
    pin: string;
    unpin: string;
    pinned: string;
    archive: string;
    rename: string;
    renameConversation: string;
  };
  appUpdate: {
    update: string;
    checking: string;
    downloading: string;
    install: string;
    installing: string;
    downloadReady: (version: string) => string;
    approvalRequired: string;
    downloadFailed: string;
    installStarted: (version: string) => string;
    installFailed: string;
  };
  settingsPanel: {
    title: string;
    deepSeekApiKey: string;
    fallbackApiKey: string;
    apiKeyPlaceholder: string;
    apiKeyConfiguredPlaceholder: string;
    apiKeyReady: string;
    fallbackApiKeyPlaceholder: string;
    soulProfile: string;
    soulProfileOpen: string;
    soulProfileClose: string;
    soulProfileModalTitle: string;
    soulProfileModalDescription: string;
    soulProfileGuides: Array<{
      title: string;
      lines: string[];
    }>;
    soulProfilePlaceholder: string;
    soulProfileSave: string;
    soulProfileSaving: string;
    soulProfileSaved: string;
    soulProfileExists: string;
    soulProfileTemplate: string;
    soulProfileSummary: string;
    soulProfileEmpty: string;
    soulProfileLoadFailed: string;
    soulProfileSaveFailed: string;
    workspaceDirectory: string;
    chooseWorkspace: string;
    saveWorkspace: string;
    balance: string;
    queryBalance: string;
    queryingBalance: string;
    balanceAvailable: string;
    balanceUnavailable: string;
    balanceNotQueried: string;
    balanceFailed: string;
    balanceEmpty: string;
    attribution: string;
  };
  controls: {
    modelRoute: string;
    largeModelProvider: string;
    accessMode: string;
    thinkingLevel: string;
    themeStyle: string;
    language: string;
    networkSearchSourceModel: string;
  };
  largeModelOptions: Record<LargeModelProvider, string>;
  modelOptions: Record<ModelRoute, string>;
  accessOptions: Record<AccessMode, string>;
  thinkingOptions: Record<ThinkingLevel, string>;
  scopeOptions: Record<WorkspaceScope, string>;
  themeOptions: Record<ThemeStyle, string>;
  networkSearchSourceOptions: Record<NetworkSearchSourceModel, string>;
  runtimePlatformOptions: Record<RuntimePlatform, string>;
  codexBridgeTransportOptions: Record<CodexBridgeTransport, string>;
  backendOptions: {
    network_search: Record<NetworkSearchBackend, string>;
    network_search_execution: Record<NetworkSearchExecutionMode, string>;
    network_search_evidence: Record<NetworkSearchEvidencePolicy, string>;
    email: Record<EmailBackend, string>;
    drive: Record<DriveBackend, string>;
    computer_screenshot: Record<ComputerScreenshotBackend, string>;
    computer_control: Record<ComputerControlBackend, string>;
  };
  backendLabels: {
    title: string;
    largeModelProvider: string;
    networkSearch: string;
    networkSearchSupport: string;
    networkSearchSourceModel: string;
    networkSearchRoute: string;
    networkSearchExecution: string;
    networkSearchEvidence: string;
    networkRequests: string;
    deepSeekOrchestration: string;
    confirmationGate: string;
    email: string;
    drive: string;
    computerScreenshot: string;
    computerControl: string;
    deepSeekApi: string;
    deepSeekChatApi: string;
    deepSeekTelemetry: string;
    apiBaseUrl: string;
    chatEndpoint: string;
    deepSeekModels: string;
    apiKeyEnv: string;
    apiKeyConfigured: string;
    apiKeyMissing: string;
    chatReady: string;
    chatNotReady: string;
    enabled: string;
    disabled: string;
    confirmationRequired: string;
    confirmationNotRequired: string;
    screenshotBackendStatus: string;
    screenshotPermission: string;
    controlBackendStatus: string;
    controlPermission: string;
    codexBridgeRuntime: string;
    backendAvailable: string;
    backendUnavailable: string;
    approvalRequired: string;
    osPermissionRequired: string;
    osPermissionNotRequired: string;
    bridgeRequired: string;
    bridgeNotRequired: string;
    bridgeEndpointConfigured: string;
    bridgeEndpointMissing: string;
    bridgeConnected: string;
    bridgeNotConnected: string;
    bridgeTransportMissing: string;
    nativeSupported: string;
    sourceModelRequired: string;
    notSelected: string;
    noTelemetry: string;
    cacheHit: string;
    cacheMiss: string;
    cacheDisabled: string;
    cacheEntries: string;
    clearCache: string;
    clearingCache: string;
    cacheCleared: (count: number) => string;
    cacheClearFailed: string;
    tokens: string;
    cost: string;
    runtimePlatform: string;
    macosPath: string;
  };
  capabilityFamilyOptions: Record<CapabilityFamily, string>;
  capabilityOptions: Record<CapabilityKind, string>;
  capabilitySummaries: Record<CapabilityKind, string>;
  riskOptions: Record<RiskLevel, string>;
  decisionOptions: Record<PolicyDecision, string>;
  accessStatusOptions: Record<CapabilityAccessStatus, string>;
  accessGrantOptions: Record<CapabilityGrantState, string>;
  invocationStatusOptions: Record<CapabilityInvocationStatus, string>;
  workbench: {
    stage: string;
    title: string;
    summary: string;
  };
  chatWorkbench: {
    title: string;
    assistantLabel: string;
    userLabel: string;
    assistantMessage: string;
    taskTitlePlaceholder: string;
    composerPlaceholder: string;
    starterPromptsLabel: string;
    quickAsk: string;
    quickDraft: string;
    quickAnalyze: string;
    saveTask: string;
    queueTask: string;
    stopTask: string;
    queueGuidance: string;
    addAttachment: string;
    removeAttachment: string;
    attachmentBlocked: string;
    attachmentMetadataOnly: string;
    attachmentDesktopOnly: string;
    attachmentAddFailed: string;
    attachmentsOnlyPrompt: string;
    guidanceQueued: string;
    guidanceRunning: string;
    guidanceQueuedFeedback: string;
    guidanceRunningFeedback: string;
    taskQueuedFeedback: string;
    stopRequestedFeedback: string;
    readyStatus: string;
    sendingStatus: string;
    pendingStages: string[];
    loopGoalDetail: string;
    loopVerifyDetail: string;
    searchPending: string;
    actionPlanLabel: string;
    missingPrerequisitesLabel: string;
    memoryCandidatesLabel: string;
    actionState: Record<
      "proposed" | "waiting_prerequisite" | "needs_confirmation" | "blocked" | "succeeded" | "failed",
      string
    >;
    emptyPrompt: string;
    desktopRuntimeMissing: string;
    deepSeekRequestFailed: string;
    deepSeekResponseReadFailed: string;
    deepSeekKeyTitle: string;
    deepSeekKeyBody: string;
    deepSeekKeyPlaceholder: string;
    deepSeekKeyRequired: string;
    workspaceTitle: string;
    workspaceBody: string;
    networkSearchTitle: string;
    networkSearchBody: string;
    approvalSummary: (count: number) => string;
    taskApprovalRejected: string;
    confirmAndRun: string;
    confirmingAction: string;
    resumeAction: string;
    resumingAction: string;
    resumeActionFailed: string;
    continue: string;
    cancel: string;
  };
  skills: {
    title: string;
    enabled: string;
    disabled: string;
    operationsTitle: string;
    operationsDescription: string;
    autoInvoke: string;
    systemSkills: string;
    installedSkills: string;
    installedPlugins: string;
    scenarioTemplates: string;
    protected: string;
    automaticUpdates: string;
    checkingUpdates: string;
    updateSummary: (checked: number, updated: number, failed: number) => string;
    installFromChat: string;
    version: string;
    source: string;
    localSource: string;
    skillType: string;
    pluginType: string;
    systemSkillType: string;
    installedTitle: string;
    manifestPlaceholder: string;
    remotePackageUrlPlaceholder: string;
    installManifest: string;
    installZip: string;
    previewRemote: string;
    installRemote: string;
    installing: string;
    empty: string;
    enable: string;
    disable: string;
    verifySource: string;
    prepareExecution: string;
    resetTrust: string;
    uninstall: string;
    executionsTitle: string;
    safeBoundary: string;
    noPermissions: string;
    installSucceeded: string;
    previewSucceeded: (name: string, fileCount: number) => string;
    statusChanged: string;
    enabledNotice: string;
    disabledNotice: string;
    sourceVerified: (provenance: string) => string;
    executionPrepared: (name: string) => string;
    executionBlocked: (name: string, reason: string) => string;
    trustReset: string;
    uninstalled: string;
    loadFailed: string;
    installFailed: string;
    previewFailed: string;
    statusFailed: string;
    sourceFailed: string;
    executionFailed: string;
    trustResetFailed: string;
    uninstallFailed: string;
    updateFailed: string;
  };
  runStatus: {
    title: string;
    current: string;
    workflowSteps: string;
    readyTitle: string;
    readyBody: string;
    runningTitle: string;
    runningBody: string;
    needsApprovalTitle: string;
    needsApprovalBody: string;
    doneTitle: string;
    doneBody: string;
    failedTitle: string;
    failedBody: string;
    agentActionTitle: string;
    agentActionBody: string;
    permissionsAndTools: string;
    routeDetails: string;
    recentRuns: string;
    recentTools: string;
    queuedRuns: string;
    runStepsLabel: (count: number) => string;
    runArtifactsLabel: (count: number) => string;
    workerLabel: (worker: string) => string;
    recoveryLabel: (count: number) => string;
    agentRunStatus: Record<AgentRunStatus, string>;
    toolStatus: Record<ToolExecutionStatus, string>;
    steps: {
      understand: string;
      attachments: string;
      evidence: string;
      memory: string;
      deepseek: string;
      guidance: string;
      validate: string;
      report: string;
    };
    stepDetails: {
      understand: string;
      attachments: (ready: number, metadataOnly: number, blocked: number) => string;
      evidence: string;
      memory: string;
      deepseek: string;
      validate: string;
      report: string;
    };
    stepState: Record<RunStatusStepState, string>;
  };
  localSetup: {
    title: string;
    required: string;
    ready: string;
    appData: string;
    settingsFile: string;
    workspaceName: string;
    workspaceNamePlaceholder: string;
    workspaceDir: string;
    workspacePlaceholder: string;
    choose: string;
    chooseFailed: string;
    workspaceDialogTitle: string;
    save: string;
    saving: string;
    saved: string;
    failed: string;
    loadFailed: string;
    managedStructure: string;
  };
  deepSeekPricing: {
    title: string;
    enabled: string;
    disabled: string;
    statusConfigured: string;
    statusNotConfigured: string;
    help: string;
    settingsFile: string;
    flashPrompt: string;
    flashCompletion: string;
    proPrompt: string;
    proCompletion: string;
    pricePlaceholder: string;
    save: string;
    saving: string;
    saved: string;
    failed: string;
    loadFailed: string;
  };
  operationsBriefing: {
    title: string;
    run: string;
    running: string;
    seedTemplates: string;
    seededTemplates: string;
    seedPendingHint: string;
    seedFailed: string;
    exportPackage: string;
    exportReport: string;
    exportHtmlReport: string;
    exportPdfReport: string;
    exported: string;
    reportExported: string;
    reportPendingHint: string;
    reportExportFailed: string;
    htmlReportExported: string;
    htmlReportPendingHint: string;
    htmlReportExportFailed: string;
    pdfReportExported: string;
    pdfReportPendingHint: string;
    pdfReportExportFailed: string;
    latestRun: string;
    runs: string;
    noRuns: string;
    pendingHint: string;
    failed: string;
    loadFailed: string;
    anomalies: string;
    actions: string;
    noAnomalies: string;
    noActions: string;
    evidence: string;
    archived: string;
    archiveEvidenceRedacted: string;
    contextReceipt: string;
    contextUserIntent: string;
    contextLoopMode: string;
    contextWorkflowPolicy: string;
    contextSelectedEvidence: string;
    contextSelectedMemories: string;
    contextMemoryRetrieval: string;
    contextMemoryScores: string;
    contextMemoryConflictHints: string;
    contextMemoryCandidateGate: string;
    contextNoSelectedMemories: string;
    contextModelRoute: string;
    contextThinkingLevel: string;
    contextTokenCache: string;
    contextValidation: string;
    contextIntentionalOmissions: string;
    contextNoItems: string;
    status: Record<OperationsBriefingRunStatus, string>;
  };
  package: {
    title: string;
    taskTitle: string;
    taskSummary: string;
    addRecord: string;
    exportPackage: string;
    copyPackage: string;
    importPackage: string;
    previewImport: string;
    previewing: string;
    previewTitle: string;
    previewReady: string;
    previewFailed: string;
    previewTotalTasks: string;
    previewNewTasks: string;
    previewSkippedTasks: string;
    previewMemoryCandidates: string;
    previewNewMemoryCandidates: string;
    previewSkippedMemoryCandidates: string;
    previewMemoryCandidateHint: string;
    previewMemoryCandidateReviewSupported: string;
    previewMemoryCandidateReviewUnsupported: string;
    previewArchivedRuns: string;
    previewNewArchivedRuns: string;
    previewSkippedArchivedRuns: string;
    previewArchiveHint: string;
    previewArchiveReplaySupported: string;
    previewArchiveReplayUnsupported: string;
    previewWorkflowTemplates: string;
    previewNewWorkflowTemplates: string;
    previewSkippedWorkflowTemplates: string;
    previewWorkflowTemplateHint: string;
    previewWorkflowTemplateImportSupported: string;
    previewWorkflowTemplateImportUnsupported: string;
    packageJson: string;
    importJson: string;
    emptyTitle: string;
    emptyImport: string;
    created: string;
    exported: string;
    copied: string;
    imported: (
      imported: number,
      skipped: number,
      candidateImported: number,
      candidateSkipped: number,
      briefingImported: number,
      briefingSkipped: number,
      templateImported: number,
      templateSkipped: number,
    ) => string;
    noRecords: string;
    copyFailed: string;
    loadFailed: string;
  };
  memory: {
    title: string;
    autoCapture: string;
    noMemories: string;
    loadFailed: string;
    search: string;
    searchPlaceholder: string;
    candidateTitle: string;
    candidateBody: string;
    candidateType: string;
    candidateScope: string;
    candidateSensitivity: string;
    candidateLifecycle: string;
    expiresAt: string;
    metadata: string;
    propose: string;
    proposing: string;
    candidates: string;
    noCandidates: string;
    accept: string;
    reject: string;
    edit: string;
    editTitle: string;
    editBody: string;
    save: string;
    saving: string;
    cancel: string;
    delete: string;
    deleting: string;
    resolving: string;
    proposed: string;
    accepted: string;
    rejected: string;
    updated: string;
    deleted: string;
    emptyCandidate: string;
    emptyEdit: string;
    emptyExpiration: string;
    proposeFailed: string;
    resolveFailed: string;
    updateFailed: string;
    deleteFailed: string;
    conflictWarning: (count: number) => string;
    conflictDetails: string;
    previewMerge: string;
    previewingMerge: string;
    mergePreviewTitle: string;
    mergePreviewDraft: string;
    mergePreviewFailed: string;
    mergeAndAccept: string;
    merged: string;
    mergeFailed: string;
    previewReplace: string;
    previewingReplace: string;
    replacePreviewTitle: string;
    replacePreviewDraft: string;
    replacePreviewTargets: string;
    replacePreviewFailed: string;
    replaceAndAccept: string;
    replaced: string;
    replaceFailed: string;
    updateAndAccept: string;
    updatedFromCandidate: string;
    updateCandidateFailed: string;
    archiveStaleTarget: string;
    archivedFromCandidate: string;
    archiveCandidateFailed: string;
    linkAndAccept: string;
    linkRelation: string;
    linkExisting: string;
    linkingExisting: string;
    linkSource: string;
    linkTarget: string;
    linkExistingNote: string;
    emptyExistingLink: string;
    existingLinked: string;
    existingLinkFailed: string;
    linked: string;
    linkFailed: string;
    linkedMemories: (count: number) => string;
    linkNote: string;
    searchMatchedBy: string;
    searchMatchUnknown: string;
    searchMatchOptions: Record<MemorySearchMatchSource, string>;
    updatedAt: string;
    candidateGate: string;
    candidatePrivacyReview: string;
    candidateSuggestedAction: string;
    candidateEvidenceExcerpt: string;
    candidateSuggestedActionOptions: Record<MemoryCandidateSuggestedAction, string>;
    candidateStatus: Record<MemoryCandidateStatus, string>;
    typeOptions: Record<MemoryType, string>;
    scopeOptions: Record<MemoryScope, string>;
    sensitivityOptions: Record<MemorySensitivity, string>;
    lifecycleOptions: Record<MemoryLifecycle, string>;
    relationOptions: Record<MemoryRelationKind, string>;
    feedbackReview: string;
    feedbackReviewEmpty: string;
    feedbackReviewCount: (count: number) => string;
    feedbackFilter: string;
    feedbackSort: string;
    feedbackFilterOptions: Record<string, string>;
    feedbackSortOptions: Record<string, string>;
    latestFeedback: string;
    needsFeedbackReview: string;
    feedbackMemoryMissing: string;
    maintenanceReview: string;
    maintenanceReviewEmpty: string;
    maintenanceReviewCount: (count: number) => string;
    maintenanceFilter: string;
    maintenanceSort: string;
    maintenanceFilterOptions: Record<string, string>;
    maintenanceSortOptions: Record<string, string>;
    maintenanceReviewKindOptions: Record<MemoryMaintenanceReviewKind, string>;
    maintenanceActionOptions: Record<MemoryMaintenanceActionKind, string>;
    maintenanceAutomatic: string;
    maintenanceNoUserAction: string;
    maintenanceSnoozeUntil: string;
    maintenanceLastAction: string;
    maintenanceQuality: string;
    maintenanceQualitySignals: string;
    maintenanceRecommendedActions: string;
    maintenanceCreateUpdateCandidate: string;
    maintenanceCandidateCreated: string;
    maintenanceRetrievalReviewed: string;
    maintenanceMarkReviewed: string;
    maintenanceSnooze: string;
    maintenanceActionRecorded: string;
    maintenanceActionFailed: string;
    maintenanceArchive: string;
    maintenanceArchiveDone: string;
    maintenanceArchiveFailed: string;
  };
  memoryFeedback: {
    title: string;
    useful: string;
    irrelevant: string;
    stale: string;
    conflicting: string;
    shouldUpdate: string;
    recorded: string;
    recordedWithMaintenance: (
      updateCandidates: number,
      retrievalReviews: number,
      autoUpdates: number,
      autoArchives: number,
      candidateDecisions: number,
      mergeCandidates: number,
      autoMerges: number,
      modelRewrites: number,
    ) => string;
    recordFailed: string;
    options: Record<MemorySelectedFeedbackKind, string>;
  };
  audit: {
    title: string;
    browser: string;
    emailSend: string;
    computerControl: string;
    empty: string;
    loadFailed: string;
    pending: string;
  };
  capabilities: {
    title: string;
    request: string;
    requesting: string;
    experimental: string;
    pendingTitle: string;
    noPending: string;
    approve: string;
    reject: string;
    resolving: string;
    loadFailed: string;
    requestFailed: string;
    resolveFailed: string;
    auditTitle: string;
  };
  browserTool: {
    title: string;
    urlPlaceholder: string;
    browse: string;
    browsing: string;
    outputTitle: string;
    noOutput: string;
    approvalRequest: string;
    pendingHint: string;
    failed: string;
  };
  browserSubmitTool: {
    title: string;
    urlPlaceholder: string;
    summaryPlaceholder: string;
    requestSubmit: string;
    requestingSubmit: string;
    pendingHint: string;
    blocked: string;
    failed: string;
  };
  networkSearchTool: {
    title: string;
    queryPlaceholder: string;
    scopePlaceholder: string;
    requestSearch: string;
    requestingSearch: string;
    pendingHint: string;
    completed: string;
    blocked: string;
    failed: string;
    sourceModelRequiredTitle: string;
    sourceModelRequiredBody: string;
    sourceModelPlaceholder: string;
    sourceModelMissing: string;
    routeNotEnabled: string;
  };
  fileTool: {
    title: string;
    pathPlaceholder: string;
    read: string;
    reading: string;
    pendingHint: string;
    failed: string;
  };
  fileWriteTool: {
    title: string;
    pathPlaceholder: string;
    summaryPlaceholder: string;
    contentPlaceholder: string;
    requestWrite: string;
    requestingWrite: string;
    pendingHint: string;
    completed: string;
    blocked: string;
    failed: string;
  };
  folderTool: {
    title: string;
    pathPlaceholder: string;
    ingest: string;
    ingesting: string;
    pendingHint: string;
    failed: string;
  };
  terminalTool: {
    title: string;
    commandLabel: string;
    run: string;
    running: string;
    pendingHint: string;
    failed: string;
    writeTitle: string;
    writeCommandLabel: string;
    writePlaceholder: string;
    requestWrite: string;
    requestingWrite: string;
    writePendingHint: string;
    writeBlocked: string;
    writeFailed: string;
    options: Record<TerminalReadCommand, string>;
  };
  computerTool: {
    title: string;
    capture: string;
    capturing: string;
    pendingHint: string;
    captured: string;
    unavailable: string;
    failed: string;
  };
  computerControlTool: {
    title: string;
    unlockTitle: string;
    unlockChallengeLabel: string;
    unlockTokenPlaceholder: string;
    unlockControl: string;
    unlockingControl: string;
    unlockReady: string;
    unlockRequired: string;
    unlockExpires: string;
    unlockFailed: string;
    targetPlaceholder: string;
    actionPlaceholder: string;
    requestControl: string;
    requestingControl: string;
    pendingHint: string;
    executed: string;
    blocked: string;
    failed: string;
  };
  emailTool: {
    title: string;
    toPlaceholder: string;
    subjectPlaceholder: string;
    bodyPlaceholder: string;
    requestSend: string;
    requestingSend: string;
    pendingHint: string;
    blocked: string;
    failed: string;
  };
  emailDraftTool: {
    title: string;
    toPlaceholder: string;
    subjectPlaceholder: string;
    bodyPlaceholder: string;
    requestDraft: string;
    requestingDraft: string;
    pendingHint: string;
    blocked: string;
    failed: string;
  };
  emailReadTool: {
    title: string;
    mailboxPlaceholder: string;
    queryPlaceholder: string;
    requestRead: string;
    requestingRead: string;
    pendingHint: string;
    blocked: string;
    failed: string;
  };
  driveReadTool: {
    title: string;
    locationPlaceholder: string;
    queryPlaceholder: string;
    requestRead: string;
    requestingRead: string;
    pendingHint: string;
    completed: string;
    blocked: string;
    failed: string;
  };
  driveWriteTool: {
    title: string;
    locationPlaceholder: string;
    summaryPlaceholder: string;
    requestWrite: string;
    requestingWrite: string;
    pendingHint: string;
    completed: string;
    blocked: string;
    failed: string;
  };
  inspector: {
    title: string;
    largeModel: string;
    model: string;
    access: string;
    thinking: string;
    scope: string;
    theme: string;
  };
};

export const translations: Record<Language, TranslationSet> = {
  zh: {
    brandTagline: "DeepSeek 本地 Agent",
    navLabel: "主导航",
    nav: {
      workbench: "对话",
      memory: "记忆",
      approvals: "审批",
      newChat: "新对话",
      conversations: "对话",
      untitledConversation: "未命名对话",
      plugins: "插件",
      settings: "设置",
      contextCompressed: "已压缩",
      pin: "置顶",
      unpin: "取消置顶",
      pinned: "已置顶",
      archive: "归档",
      rename: "重命名",
      renameConversation: "重命名对话",
    },
    appUpdate: {
      update: "更新",
      checking: "检查更新",
      downloading: "下载更新",
      install: "安装更新",
      installing: "正在更新",
      downloadReady: (version) => `${version} 已下载，点击安装更新`,
      approvalRequired: "请先在右侧批准此更新步骤，然后再次点击更新。",
      downloadFailed: "更新下载失败",
      installStarted: (version) => `正在静默安装 ${version} 并重启 DS Agent`,
      installFailed: "更新启动失败",
    },
    settingsPanel: {
      title: "设置",
      deepSeekApiKey: "DeepSeek API key",
      fallbackApiKey: "备用 DeepSeek API key",
      apiKeyPlaceholder: "输入主 key，当前窗口内使用",
      apiKeyConfiguredPlaceholder: "••••••••••••••••（已配置）",
      apiKeyReady: "API key 已通过启动检测",
      fallbackApiKeyPlaceholder: "主 key 不可用时自动尝试",
      soulProfile: "Soul",
      soulProfileOpen: "打开 Soul 设置",
      soulProfileClose: "关闭",
      soulProfileModalTitle: "Soul Profile",
      soulProfileModalDescription:
        "只写长期稳定的称呼、语气和协作偏好。不要写密码、密钥、账号、身份证件或临时任务内容。",
      soulProfileGuides: [
        {
          title: "User",
          lines: [
            "preferred_name：你希望 DS Agent 记住的名字或简称。",
            "address_as：默认怎么称呼你，例如“李总”或直接称呼名字。",
            "language_preferences：默认使用中文、英文或双语。",
            "default_response_tone：默认回复语气，例如简洁、温暖、正式。",
            "default_response_length：默认长短，例如短答、适中、详细。",
            "formatting_preferences：默认格式偏好，例如少用嵌套列表。",
            "initiative_level：DS Agent 可以主动推进到什么程度。",
          ],
        },
        {
          title: "DS Agent",
          lines: [
            "user_calls_ds_agent：你平时怎么称呼这个工具。",
            "ds_agent_should_refer_to_itself_as：DS Agent 回复时怎么称呼自己。",
            "relationship_boundary：协作边界，例如只做本地执行和可审计记忆。",
          ],
        },
        {
          title: "Stable Preferences",
          lines: [
            "workflow_preferences：长期工作流偏好，例如先验收再发布。",
            "writing_preferences：长期写作偏好，例如中文公文或英文邮件风格。",
            "confirmation_preferences：哪些动作需要先问你，哪些可直接做。",
            "privacy_preferences：隐私和本地文件处理偏好。",
          ],
        },
        {
          title: "Never Store",
          lines: [
            "这一段是禁止写入清单，用来提醒 DS Agent 不要保存密钥、密码、账号或敏感身份信息。",
            "临时任务、一次性文件路径、实时价格和当天状态也不要写进 Soul Profile。",
          ],
        },
      ],
      soulProfilePlaceholder: "编辑 memory/soul.md，记录称呼、DS Agent 称呼和默认回复偏好。",
      soulProfileSave: "保存 soul profile",
      soulProfileSaving: "保存中",
      soulProfileSaved: "Soul Profile 已保存。",
      soulProfileExists: "已启用本机 Soul Profile",
      soulProfileTemplate: "尚未保存，当前显示模板",
      soulProfileSummary: "本轮可用摘要",
      soulProfileEmpty: "Soul Profile 内容不能为空。",
      soulProfileLoadFailed: "Soul Profile 加载失败。",
      soulProfileSaveFailed: "Soul Profile 保存失败。",
      workspaceDirectory: "工作目录",
      chooseWorkspace: "选择目录",
      saveWorkspace: "保存目录",
      balance: "DeepSeek 余额",
      queryBalance: "读取真实余额",
      queryingBalance: "正在读取",
      balanceAvailable: "余额可用",
      balanceUnavailable: "余额不足或不可用",
      balanceNotQueried: "尚未读取余额",
      balanceFailed: "DeepSeek 余额读取失败。",
      balanceEmpty: "DeepSeek 未返回余额明细。",
      attribution: "DS Agent 由 Lee take 创建并维护。",
    },
    controls: {
      modelRoute: "模型类型",
      largeModelProvider: "大模型",
      accessMode: "访问权限",
      thinkingLevel: "思考强度",
      themeStyle: "界面风格",
      language: "界面语言",
      networkSearchSourceModel: "联网搜索来源模型",
    },
    largeModelOptions: {
      deepseek: "DeepSeek",
      chatgpt: "外部对话模型",
      codex: "本地桥接路线",
      custom: "自定义模型",
    },
    modelOptions: {
      auto: "DeepSeek 自动",
      flash: "Flash",
      pro: "Pro",
    },
    accessOptions: {
      ask_every_step: "每步询问",
      ask_on_risk: "风险时询问",
      limited_auto: "有限自动",
      full_access: "完全访问",
    },
    thinkingOptions: {
      auto: "自动思考",
      fast: "快速",
      standard: "标准",
      deep: "深入",
    },
    scopeOptions: {
      workspace: "工作区",
    },
    themeOptions: {
      ink: "水墨山水",
      porcelain: "青花瓷",
    },
    networkSearchSourceOptions: {
      free_web_source: "免费网页来源模型",
      free_local_browser: "免费本地浏览器搜索（alpha）",
      free_source_aggregator: "免费来源聚合器（alpha）",
    },
    runtimePlatformOptions: {
      windows: "Windows",
      macos: "macOS",
      other: "其它平台",
    },
    codexBridgeTransportOptions: {
      http: "本地 HTTP 桥接服务",
      stdio: "本地服务启动暂缓",
    },
    backendOptions: {
      network_search: {
        deepseek: "DeepSeek 搜索编排",
        native_large_model: "大模型原生联网搜索",
        source_backed_model: "来源支撑联网搜索模型",
      },
      network_search_execution: {
        permission_audit_only: "仅权限复核",
        source_backed_adapter: "来源关联路线执行",
        native_bridge_contract: "所选模型联网搜索",
      },
      network_search_evidence: {
        pending_user_confirmation: "待确认",
        source_links_required: "必须保留来源链接",
      },
      email: {
        architecture_only: "仅保留架构",
      },
      drive: {
        local_folder_export_package: "本地文件夹与导出包",
      },
      computer_screenshot: {
        codex_style_screen_capture: "兼容本地服务屏幕路线",
        codex_bridge_screen_capture: "已配置本地服务屏幕路线",
        local_windows_screen_capture: "本地 Windows 屏幕查看",
        local_macos_screen_capture: "本地 macOS 屏幕查看",
      },
      computer_control: {
        codex_style_input_control: "兼容本地服务鼠标键盘路线",
        codex_bridge_input_control: "已配置本地服务鼠标键盘路线",
        local_windows_input_control: "本地 Windows 鼠标键盘控制",
        local_macos_input_control: "本地 macOS 鼠标键盘控制",
      },
    },
    backendLabels: {
      title: "工具路线策略",
      largeModelProvider: "大模型",
      networkSearch: "联网搜索",
      networkSearchSupport: "搜索支持",
      networkSearchSourceModel: "搜索来源模型",
      networkSearchRoute: "搜索路线",
      networkSearchExecution: "搜索执行",
      networkSearchEvidence: "证据策略",
      networkRequests: "真实联网",
      deepSeekOrchestration: "DeepSeek 编排",
      confirmationGate: "确认门",
      email: "邮件",
      drive: "本地文件夹",
      computerScreenshot: "屏幕读取",
      computerControl: "电脑控制",
      deepSeekApi: "DeepSeek API",
      deepSeekChatApi: "DeepSeek Chat API",
      deepSeekTelemetry: "DeepSeek 遥测",
      apiBaseUrl: "API 地址",
      chatEndpoint: "Chat 接口",
      deepSeekModels: "DeepSeek 模型",
      apiKeyEnv: "Key 环境变量",
      apiKeyConfigured: "已配置",
      apiKeyMissing: "未配置",
      chatReady: "已就绪",
      chatNotReady: "未就绪",
      enabled: "已启用",
      disabled: "未启用",
      confirmationRequired: "需确认",
      confirmationNotRequired: "无需确认",
      screenshotBackendStatus: "屏幕路线状态",
      screenshotPermission: "屏幕权限",
      controlBackendStatus: "控制路线状态",
      controlPermission: "控制权限",
      codexBridgeRuntime: "本地桥接服务",
      backendAvailable: "可用",
      backendUnavailable: "不可用",
      approvalRequired: "需审批",
      osPermissionRequired: "需系统授权",
      osPermissionNotRequired: "无需显式系统授权",
      bridgeRequired: "需要本地桥接",
      bridgeNotRequired: "不需要本地桥接",
      bridgeEndpointConfigured: "本地地址已配置",
      bridgeEndpointMissing: "本地地址未配置",
      bridgeConnected: "已连接",
      bridgeNotConnected: "未连接",
      bridgeTransportMissing: "路线类型未选择",
      nativeSupported: "原生支持",
      sourceModelRequired: "需来源模型",
      notSelected: "未选择",
      noTelemetry: "暂无调用",
      cacheHit: "缓存命中",
      cacheMiss: "缓存未命中",
      cacheDisabled: "缓存关闭",
      cacheEntries: "缓存条目",
      clearCache: "清空缓存",
      clearingCache: "清空中",
      cacheCleared: (count) => `已清空 ${count} 条缓存。`,
      cacheClearFailed: "DeepSeek 缓存清空失败。",
      tokens: "tokens",
      cost: "费用",
      runtimePlatform: "运行平台",
      macosPath: "macOS 路径",
    },
    capabilityFamilyOptions: {
      file: "文件",
      network: "联网",
      browser: "浏览器",
      email: "邮件",
      drive: "本地文件夹",
      terminal: "终端",
      computer_use: "电脑控制",
      app_update: "应用更新",
      skill: "Skill",
    },
    capabilityOptions: {
      file_read: "读取文件",
      file_write: "写入文件",
      network_search: "联网搜索",
      browser_browse: "浏览网页",
      browser_submit: "提交网页",
      email_read: "读取邮件",
      email_draft: "起草邮件",
      email_send: "发送邮件",
      connector_attachment_read: "下载连接账户附件",
      connector_write: "修改连接账户",
      drive_read: "读取本地文件夹",
      drive_write: "导出到本地文件夹",
      terminal_read: "读取终端",
      terminal_write: "写入终端",
      computer_screenshot: "屏幕截图",
      computer_control: "控制电脑",
      app_update_check: "检查应用更新",
      app_update_download: "下载应用更新",
      app_update_install: "安装应用更新",
      skill_use: "使用声明式 Skill",
    },
    capabilitySummaries: {
      file_read: "读取工作区内经授权的本地文件。",
      file_write: "在授权范围内创建或修改草稿与导出物。",
      network_search: "联网检索公开资料并保留来源。",
      browser_browse: "打开并检查网页内容。",
      browser_submit: "填写或提交网页表单，默认需要审批。",
      email_read: "读取选定邮件线程作为任务证据。",
      email_draft: "生成邮件草稿，不直接发送。",
      email_send: "发送已审批的外发邮件。",
      connector_attachment_read: "经逐项审批，将一个连接账户附件作为不受信任证据下载到托管工作区。",
      connector_write: "经明确审批后发送或修改连接账户中的内容。",
      drive_read: "读取选定本地文件夹中的文件。",
      drive_write: "把成果导出到选定本地文件夹。",
      terminal_read: "运行只读诊断命令并收集输出。",
      terminal_write: "运行可能修改文件或系统状态的命令。",
      computer_screenshot: "截取或检查当前屏幕。",
      computer_control: "执行鼠标和键盘动作，每步审批。",
      app_update_check: "读取可信发布信息并与当前版本比较。",
      app_update_download: "把可信安装包下载到隔离的更新目录。",
      app_update_install: "经明确审批后启动已验证安装包并重启应用。",
      skill_use: "加载已安装、已启用且通过哈希校验的声明式 Skill 入口。",
    },
    riskOptions: {
      low: "低风险",
      medium: "中风险",
      high: "高风险",
      critical: "关键风险",
    },
    decisionOptions: {
      allow: "允许",
      ask: "询问",
      deny: "拒绝",
    },
    accessStatusOptions: {
      auto_approved: "自动通过",
      pending_approval: "待审批",
      approved: "已批准",
      rejected: "已拒绝",
      denied: "已拦截",
    },
    accessGrantOptions: {
      not_granted: "未授权",
      reusable: "可复用",
      one_shot_available: "一次可用",
      one_shot_consumed: "已消耗",
    },
    invocationStatusOptions: {
      succeeded: "成功",
      pending_approval: "待审批",
      failed: "失败",
    },
    workbench: {
      stage: "0.1.0 预览",
      title: "DeepSeek 对话工作台",
      summary:
        "中间用于直接对话和结果回看，左侧管理技能、插件与设置，右侧显示执行状态。",
    },
    chatWorkbench: {
      title: "DeepSeek 对话工作台",
      assistantLabel: "DS Agent",
      userLabel: "你",
      assistantMessage: "直接输入问题或指令。我会先判断需要的上下文、权限和插件，再连接 DeepSeek 回复。",
      taskTitlePlaceholder: "给这个任务起个短标题",
      composerPlaceholder: "输入问题、文字或指令",
      starterPromptsLabel: "试试这些办公任务",
      quickAsk: "根据我的证据文件夹，生成一份经营简报。",
      quickDraft: "把这段会议纪要整理成行动项、责任部门、截止时间和风险提示。",
      quickAnalyze: "继续上次的项目，先说明你用了哪些记忆，再给我下一步建议。",
      saveTask: "发送",
      queueTask: "加入后台队列",
      stopTask: "停止",
      queueGuidance: "补充到当前任务",
      addAttachment: "添加文件",
      removeAttachment: "移除附件",
      attachmentBlocked: "已阻止",
      attachmentMetadataOnly: "仅元数据",
      attachmentDesktopOnly: "请在 DS Agent 桌面窗口中添加本地文件或图片。",
      attachmentAddFailed: "附件添加失败，请重新选择文件。",
      attachmentsOnlyPrompt: "请根据本次附加的文件和图片继续处理。",
      guidanceQueued: "补充说明已收到，当前小节点完成后会继续引导",
      guidanceRunning: "正在引导补充指令，并入同一任务继续执行",
      guidanceQueuedFeedback: "已收到补充说明，当前小节点完成后会并入同一任务。",
      guidanceRunningFeedback: "正在引导补充指令，DS Agent 会把它和当前任务统一考虑。",
      taskQueuedFeedback: "已加入后台队列；当前任务完成后会自动执行。",
      stopRequestedFeedback: "已请求停止当前任务；本轮返回将不再追加到对话。",
      readyStatus: "输入后会先预处理，再连接 DeepSeek",
      sendingStatus: "正在预处理并连接 DeepSeek",
      pendingStages: [
        "正在预处理指令并检查本地状态",
        "已提交给 DeepSeek，正在等待结构化回复",
        "DeepSeek 仍在生成，DS Agent 会继续等待并校验动作",
        "请求仍在进行中，本地程序没有宕机",
      ],
      loopGoalDetail: "建立目标契约、完成标准和边界",
      loopVerifyDetail: "按用户目标验证真实结果",
      searchPending: "我已发起网络搜索权限请求，确认后可以继续。",
      actionPlanLabel: "DS Agent 待处理动作",
      missingPrerequisitesLabel: "需要补充",
      memoryCandidatesLabel: "候选记忆已进入复核",
      actionState: {
        proposed: "待执行",
        waiting_prerequisite: "待补充",
        needs_confirmation: "需确认",
        blocked: "已阻止",
        succeeded: "已执行",
        failed: "失败",
      },
      emptyPrompt: "请输入问题或指令。",
      desktopRuntimeMissing: "当前是浏览器预览，未连接 DS Agent 桌面内核。请在 Tauri 桌面窗口中运行后再连接 DeepSeek。",
      deepSeekRequestFailed: "DeepSeek 请求失败，DS Agent 未执行任何本地动作。请稍后重试。",
      deepSeekResponseReadFailed: "DeepSeek 本次返回内容读取失败，DS Agent 未执行任何本地动作。请稍后重试。",
      deepSeekKeyTitle: "连接 DeepSeek",
      deepSeekKeyBody: "请输入 DeepSeek API key。本次先保存在当前会话内存中，不写入源码或本地文件。",
      deepSeekKeyPlaceholder: "sk-...",
      deepSeekKeyRequired: "请先输入 DeepSeek API key。",
      workspaceTitle: "设置工作目录",
      workspaceBody: "请选择一个工作目录。DS Agent 会在里面自动创建和维护项目结构。",
      networkSearchTitle: "配置网络搜索",
      networkSearchBody: "这条指令看起来需要网络信息。请选择搜索来源模型后继续；需要密钥的搜索源会走对应提供商配置。",
      approvalSummary: (count) =>
        `本次任务需要确认以下 ${count} 项动作；一次确认后将按顺序执行，也可以拒绝整个任务。`,
      taskApprovalRejected: "用户拒绝了本次任务的待授权动作。",
      confirmAndRun: "确认并执行",
      confirmingAction: "执行中",
      resumeAction: "继续执行",
      resumingAction: "执行中",
      resumeActionFailed: "批准后继续执行失败。",
      continue: "继续",
      cancel: "取消",
    },
    skills: {
      title: "技能与插件",
      enabled: "已启用",
      disabled: "已禁用",
      operationsTitle: "运营简报",
      operationsDescription: "读取本地证据，调用 DeepSeek 生成可复核的经营管理简报。",
      autoInvoke: "DS Agent 会在任务需要时自动调用已安装的技能与插件。",
      systemSkills: "系统技能",
      installedSkills: "已安装 Skill",
      installedPlugins: "已安装插件",
      scenarioTemplates: "场景模板",
      protected: "系统保护",
      automaticUpdates: "启动时后台自动检查并升级",
      checkingUpdates: "正在后台检查插件更新…",
      updateSummary: (checked, updated, failed) =>
        `已检查 ${checked} 项，升级 ${updated} 项，失败 ${failed} 项。`,
      installFromChat: "把 GitHub 或 Hugging Face 地址直接发到聊天中，DS Agent 会自动安装。",
      version: "版本",
      source: "来源",
      localSource: "本地生成",
      skillType: "Skill",
      pluginType: "插件",
      systemSkillType: "系统 Skill",
      installedTitle: "本地 Skill Registry",
      manifestPlaceholder: "粘贴 ds-agent.skill.v1 manifest JSON。本版本只登记声明式 skill，不执行脚本或二进制。",
      remotePackageUrlPlaceholder: "GitHub/Hugging Face Skill zip URL",
      installManifest: "安装 manifest",
      installZip: "导入 zip",
      previewRemote: "预检远程包",
      installRemote: "安装远程包",
      installing: "正在校验",
      empty: "还没有安装本地 skill。",
      enable: "启用",
      disable: "禁用",
      verifySource: "验证来源",
      prepareExecution: "准备执行",
      resetTrust: "重置信任",
      uninstall: "卸载",
      executionsTitle: "执行审计",
      safeBoundary: "仅校验并登记 manifest；脚本、二进制和高风险权限默认阻止。",
      noPermissions: "未声明权限",
      installSucceeded: "Skill manifest 已通过校验并登记。",
      previewSucceeded: (name, fileCount) => `远程包预检通过：${name}，${fileCount} 个文件。`,
      statusChanged: "Skill 状态已记录。",
      enabledNotice: "已启用，后续任务可以自动调用。",
      disabledNotice: "已禁用，不再参与自动调用。",
      sourceVerified: (provenance: string) => `来源已验证：${provenance}`,
      executionPrepared: (name: string) => `已准备安全执行计划：${name}`,
      executionBlocked: (name: string, reason: string) => `已阻止 skill 执行：${name}（${reason}）`,
      trustReset: "Skill 信任已重置并禁用，需复核后再启用。",
      uninstalled: "Skill 已卸载，审计记录已保留。",
      loadFailed: "读取本地 skill registry 失败。",
      installFailed: "Skill manifest 未通过安全校验。",
      previewFailed: "远程 skill 包预检失败。",
      statusFailed: "Skill 状态更新失败。",
      sourceFailed: "Skill 来源验证失败。",
      executionFailed: "Skill 执行准备失败。",
      trustResetFailed: "Skill 信任重置失败。",
      uninstallFailed: "Skill 卸载失败。",
      updateFailed: "后台自动升级检查失败。",
    },
    runStatus: {
      title: "任务状态",
      current: "当前状态",
      workflowSteps: "运行步骤",
      readyTitle: "等待你的指令",
      readyBody: "在中间对话框输入问题或指令；需要工具、权限或工作流时，这里会显示执行状态。",
      runningTitle: "正在执行工作流",
      runningBody: "DS Agent 正在读取证据、组装上下文并调用 DeepSeek。",
      needsApprovalTitle: "等待你的确认",
      needsApprovalBody: "有权限请求需要处理；确认后可继续运行。",
      doneTitle: "草稿已准备好",
      doneBody: "工作流已生成可复核的结果，可在中间区域查看和导出。",
      failedTitle: "运行遇到问题",
      failedBody: "查看工具记录或错误提示后，可以修正输入再重试。",
      agentActionTitle: "已解析 Agent 动作",
      agentActionBody: "DeepSeek 返回了结构化计划，DS Agent 已将用户回复和本地动作提案分离。",
      permissionsAndTools: "权限与工具",
      routeDetails: "模型与路线详情",
      recentRuns: "最近运行",
      recentTools: "最近工具执行",
      queuedRuns: "排队中",
      runStepsLabel: (count) => `${count} 个步骤`,
      runArtifactsLabel: (count) => `${count} 个产物`,
      workerLabel: (worker) => `Worker：${worker}`,
      recoveryLabel: (count) => `已恢复 ${count} 次`,
      agentRunStatus: {
        queued: "排队中",
        running: "执行中",
        waiting_for_prerequisite: "等待前置条件",
        waiting_for_confirmation: "等待确认",
        blocked: "已阻止",
        cancel_requested: "正在取消",
        completed: "已完成",
        failed: "失败",
        cancelled: "已取消",
      },
      toolStatus: {
        waiting_for_confirmation: "待批准",
        running: "执行中",
        succeeded: "已验证",
        failed: "失败",
        blocked: "已阻止",
      },
      steps: {
        understand: "理解任务",
        attachments: "附件证据",
        evidence: "读取证据",
        memory: "选择记忆",
        deepseek: "调用 DeepSeek",
        guidance: "补充指令",
        validate: "校验结果",
        report: "生成与导出",
      },
      stepDetails: {
        understand: "等待你在对话框输入问题或指令。",
        attachments: (ready, metadataOnly, blocked) =>
          `当前任务附件：${ready} 个可用，${metadataOnly} 个仅元数据，${blocked} 个已阻止。`,
        evidence: "需要时会在工作目录中读取或生成证据。",
        memory: "需要时会从本地记忆中选择上下文。",
        deepseek: "等待 DeepSeek Chat 就绪。",
        validate: "等待生成后进行结果校验。",
        report: "结果完成后可导出报告或工作包。",
      },
      stepState: {
        done: "已完成",
        current: "进行中",
        waiting: "等待",
        needs_action: "需确认",
        blocked: "受阻",
      },
    },
    localSetup: {
      title: "工作目录",
      required: "首次运行需要设置本机工作目录。",
      ready: "工作目录已设置。",
      appData: "应用数据目录",
      settingsFile: "目录配置",
      workspaceName: "工作区名称",
      workspaceNamePlaceholder: "例如：南昌福朋经营分析",
      workspaceDir: "工作目录",
      workspacePlaceholder: "选择或输入一个本机工作目录",
      choose: "选择",
      chooseFailed: "打开目录选择器失败。",
      workspaceDialogTitle: "选择工作目录",
      save: "保存工作目录",
      saving: "保存中",
      saved: "工作目录已保存，项目结构会自动维护。",
      failed: "工作目录保存失败。",
      loadFailed: "工作目录加载失败。",
      managedStructure:
        "DS Agent 会在工作目录内自动管理证据、导出、报告、运行记录、来源、工作包、记忆和日志。",
    },
    deepSeekPricing: {
      title: "DeepSeek 价格表",
      enabled: "启用成本估算",
      disabled: "未启用成本估算",
      statusConfigured: "已配置本地价格表",
      statusNotConfigured: "未配置本地价格表",
      help: "按 USD / 1M tokens 填写。价格保存在本机应用数据目录，不写死到开源代码。",
      settingsFile: "价格配置",
      flashPrompt: "Flash 输入",
      flashCompletion: "Flash 输出",
      proPrompt: "Pro 输入",
      proCompletion: "Pro 输出",
      pricePlaceholder: "例如 0.14",
      save: "保存价格表",
      saving: "保存中",
      saved: "DeepSeek 价格表已保存。",
      failed: "DeepSeek 价格表保存失败。",
      loadFailed: "DeepSeek 价格表加载失败。",
    },
    operationsBriefing: {
      title: "运营简报工作流",
      run: "运行",
      running: "运行中",
      seedTemplates: "写入模板",
      seededTemplates: "空白操作模板已写入本地证据文件夹。",
      seedPendingHint: "已创建文件写入审批请求，批准后再次写入即可生成模板。",
      seedFailed: "空白操作模板写入失败。",
      exportPackage: "导出简报包",
      exportReport: "导出报告",
      exportHtmlReport: "导出 HTML",
      exportPdfReport: "导出 PDF",
      exported: "简报工作包已生成。",
      reportExported: "运营简报 Markdown 报告已导出到本地导出文件夹。",
      reportPendingHint: "已创建本地文件夹导出审批请求，批准后再次导出即可写入报告。",
      reportExportFailed: "运营简报报告导出失败。",
      htmlReportExported: "运营简报 HTML 报告已导出到本地导出文件夹。",
      htmlReportPendingHint: "已创建本地文件夹导出审批请求，批准后再次导出即可写入 HTML 报告。",
      htmlReportExportFailed: "运营简报 HTML 报告导出失败。",
      pdfReportExported: "运营简报 PDF 报告已导出到本地导出文件夹。",
      pdfReportPendingHint: "已创建本地文件夹导出审批请求，批准后再次导出即可写入 PDF 报告。",
      pdfReportExportFailed: "运营简报 PDF 报告导出失败。",
      latestRun: "最近运行",
      runs: "运行归档",
      noRuns: "暂无运营简报运行记录",
      pendingHint: "已创建待审批请求，批准后再次运行即可执行。",
      failed: "运营简报运行失败。",
      loadFailed: "运营简报运行记录加载失败。",
      anomalies: "异常线索",
      actions: "行动项",
      noAnomalies: "暂无异常线索",
      noActions: "暂无行动项",
      evidence: "证据",
      archived: "归档",
      archiveEvidenceRedacted: "源机器证据句柄已清理",
      contextReceipt: "上下文回执",
      contextUserIntent: "用户意图",
      contextLoopMode: "循环模式",
      contextWorkflowPolicy: "工作流策略",
      contextSelectedEvidence: "选入证据",
      contextSelectedMemories: "选入记忆",
      contextMemoryRetrieval: "记忆检索",
      contextMemoryScores: "记忆评分",
      contextMemoryConflictHints: "记忆提示",
      contextMemoryCandidateGate: "候选闸门",
      contextNoSelectedMemories: "本次未选入记忆",
      contextModelRoute: "模型类型",
      contextThinkingLevel: "思考等级",
      contextTokenCache: "Token / 缓存",
      contextValidation: "验证结果",
      contextIntentionalOmissions: "有意省略",
      contextNoItems: "无",
      status: {
        pending_approval: "待审批",
        draft_ready: "草稿就绪",
        failed: "失败",
      },
    },
    package: {
      title: "任务记录与工作包",
      taskTitle: "任务标题",
      taskSummary: "任务摘要",
      addRecord: "记录任务",
      exportPackage: "导出工作包",
      copyPackage: "复制",
      importPackage: "导入",
      previewImport: "预览",
      previewing: "预览中",
      previewTitle: "导入预览",
      previewReady: "工作包预览已生成。",
      previewFailed: "工作包预览失败。",
      previewTotalTasks: "任务总数",
      previewNewTasks: "新增任务",
      previewSkippedTasks: "跳过任务",
      previewMemoryCandidates: "候选记忆",
      previewNewMemoryCandidates: "新增候选记忆",
      previewSkippedMemoryCandidates: "跳过候选记忆",
      previewMemoryCandidateHint: "候选记忆将进入本地确认队列，不会直接写入长期记忆。",
      previewMemoryCandidateReviewSupported: "候选记忆可进入本地复核队列。",
      previewMemoryCandidateReviewUnsupported: "当前版本无法复核这些候选记忆。",
      previewArchivedRuns: "简报归档",
      previewNewArchivedRuns: "新增简报归档",
      previewSkippedArchivedRuns: "跳过简报归档",
      previewArchiveHint: "简报运行将作为只读归档导入，不会重新执行工具。",
      previewArchiveReplaySupported: "简报归档可在本地只读回放。",
      previewArchiveReplayUnsupported: "当前版本无法回放这些简报归档。",
      previewWorkflowTemplates: "工作流模板",
      previewNewWorkflowTemplates: "新增模板",
      previewSkippedWorkflowTemplates: "跳过模板",
      previewWorkflowTemplateHint: "工作流模板包将登记为本地可用资产，不会写入用户目录。",
      previewWorkflowTemplateImportSupported: "工作流模板可登记为本地可用资产。",
      previewWorkflowTemplateImportUnsupported: "当前版本无法导入这些工作流模板。",
      packageJson: "工作包 JSON",
      importJson: "导入 JSON",
      emptyTitle: "请先填写任务标题。",
      emptyImport: "请先粘贴工作包 JSON。",
      created: "任务记录已写入本地事件库。",
      exported: "工作包已生成。",
      copied: "工作包 JSON 已复制。",
      imported: (
        imported,
        skipped,
        candidateImported,
        candidateSkipped,
        briefingImported,
        briefingSkipped,
        templateImported,
        templateSkipped,
      ) =>
        `导入完成：任务新增 ${imported} 条，跳过 ${skipped} 条；候选记忆新增 ${candidateImported} 条，跳过 ${candidateSkipped} 条；简报归档新增 ${briefingImported} 条，跳过 ${briefingSkipped} 条；工作流模板新增 ${templateImported} 个，跳过 ${templateSkipped} 个。`,
      noRecords: "暂无任务记录",
      copyFailed: "复制失败，请手动选择 JSON。",
      loadFailed: "任务记录加载失败。",
    },
    memory: {
      title: "自动记忆",
      autoCapture: "由任务记录自动沉淀",
      noMemories: "暂无自动记忆",
      loadFailed: "记忆加载失败。",
      search: "搜索",
      searchPlaceholder: "搜索记忆",
      candidateTitle: "候选记忆标题",
      candidateBody: "候选记忆内容",
      candidateType: "类型",
      candidateScope: "范围",
      candidateSensitivity: "敏感度",
      candidateLifecycle: "生命周期",
      expiresAt: "过期日期",
      metadata: "记忆元数据",
      propose: "提议记忆",
      proposing: "提议中",
      candidates: "候选记忆",
      noCandidates: "暂无候选记忆",
      accept: "后台接受",
      reject: "后台拒绝",
      edit: "编辑",
      editTitle: "记忆标题",
      editBody: "记忆内容",
      save: "保存",
      saving: "保存中",
      cancel: "取消",
      delete: "删除",
      deleting: "删除中",
      resolving: "处理中",
      proposed: "候选记忆已提交，等待确认。",
      accepted: "候选记忆已由后台写入长期记忆。",
      rejected: "候选记忆已拒绝。",
      updated: "长期记忆已更新。",
      deleted: "长期记忆已删除。",
      emptyCandidate: "请填写候选记忆标题和内容。",
      emptyEdit: "请填写记忆标题和内容。",
      emptyExpiration: "请选择过期日期。",
      proposeFailed: "候选记忆提交失败。",
      resolveFailed: "候选记忆处理失败。",
      updateFailed: "长期记忆更新失败。",
      deleteFailed: "长期记忆删除失败。",
      conflictWarning: (count) => `可能与 ${count} 条长期记忆重叠`,
      conflictDetails: "重叠记忆",
      previewMerge: "预览合并",
      previewingMerge: "生成中",
      mergePreviewTitle: "合并草稿",
      mergePreviewDraft: "草稿内容",
      mergePreviewFailed: "合并草稿生成失败。",
      mergeAndAccept: "后台合并",
      merged: "候选记忆已合并，旧记忆已归档。",
      mergeFailed: "候选记忆合并失败。",
      previewReplace: "预览替换",
      previewingReplace: "生成中",
      replacePreviewTitle: "替换预览",
      replacePreviewDraft: "替换草稿",
      replacePreviewTargets: "将被替换",
      replacePreviewFailed: "替换预览生成失败。",
      replaceAndAccept: "后台替换",
      replaced: "候选记忆已接受，被替换记忆已归档。",
      replaceFailed: "候选记忆替换失败。",
      updateAndAccept: "更新已有记忆",
      updatedFromCandidate: "候选记忆已接受，已有记忆已更新。",
      updateCandidateFailed: "候选记忆更新失败。",
      archiveStaleTarget: "归档过时目标",
      archivedFromCandidate: "候选记忆已接受，过时记忆已归档。",
      archiveCandidateFailed: "候选记忆归档失败。",
      linkAndAccept: "后台关联",
      linkRelation: "关联关系",
      linkExisting: "关联已有记忆",
      linkingExisting: "关联中",
      linkSource: "源记忆",
      linkTarget: "目标记忆",
      linkExistingNote: "为什么关联这两条记忆",
      emptyExistingLink: "请选择两条不同的长期记忆。",
      existingLinked: "两条长期记忆已建立关联。",
      existingLinkFailed: "长期记忆关联失败。",
      linked: "候选记忆已接受，并与重叠记忆建立关联。",
      linkFailed: "候选记忆关联失败。",
      linkedMemories: (count) => `关联 ${count} 条记忆`,
      linkNote: "关联说明",
      searchMatchedBy: "命中来源",
      searchMatchUnknown: "关联记忆",
      searchMatchOptions: {
        direct: "直接命中",
        linked_memory_title: "关联标题命中",
        linked_memory_body: "关联正文命中",
      },
      updatedAt: "更新于",
      candidateGate: "候选闸门",
      candidatePrivacyReview: "隐私检查",
      candidateSuggestedAction: "建议动作",
      candidateEvidenceExcerpt: "证据摘录",
      candidateSuggestedActionOptions: {
        new: "新增",
        update: "更新",
        merge: "合并",
        replace: "替换",
        archive: "归档",
        link: "关联",
        reject_hint: "建议拒绝",
      },
      candidateStatus: {
        pending: "待确认",
        accepted: "已接受",
        rejected: "已拒绝",
      },
      typeOptions: {
        preference: "偏好",
        project_context: "项目上下文",
        workflow_rule: "工作流规则",
        artifact: "成果物",
        failure_pattern: "失败模式",
      },
      scopeOptions: {
        workspace: "工作区",
        project: "项目",
        organization: "组织",
        user: "用户",
      },
      sensitivityOptions: {
        normal: "普通",
        sensitive: "敏感",
      },
      lifecycleOptions: {
        active: "启用",
        archived: "归档",
        expires: "会过期",
      },
      relationOptions: {
        related: "相关",
        updates: "更新",
        extends: "扩展",
        derives: "推导",
      },
      feedbackReview: "反馈复核",
      feedbackReviewEmpty: "暂无已记录的记忆反馈",
      feedbackReviewCount: (count) => `${count} 条反馈`,
      feedbackFilter: "反馈筛选",
      feedbackSort: "反馈排序",
      feedbackFilterOptions: {
        all: "全部反馈",
        needs_review: "需要复核",
        useful: "有用",
        irrelevant: "无关",
        stale: "过时",
        conflicting: "冲突",
        should_update: "应更新",
      },
      feedbackSortOptions: {
        priority: "优先级",
        latest: "最新反馈",
        feedback_count: "反馈数量",
      },
      latestFeedback: "最新反馈",
      needsFeedbackReview: "需要复核",
      feedbackMemoryMissing: "这条记忆当前不可见",
      maintenanceReview: "后台维护审计",
      maintenanceReviewEmpty: "暂无需要展示的后台维护记录",
      maintenanceReviewCount: (count) => `${count} 条维护线索`,
      maintenanceFilter: "维护筛选",
      maintenanceSort: "维护排序",
      maintenanceFilterOptions: {
        all: "全部维护记录",
        needs_review: "需要确认",
        retrieval: "检索调优",
        update_archive: "更新/归档",
        conflict: "冲突",
        snoozed: "稍后处理",
      },
      maintenanceSortOptions: {
        priority: "优先级",
        latest: "最新反馈",
        feedback_count: "反馈数量",
      },
      maintenanceReviewKindOptions: {
        retrieval: "检索调优",
        update_archive: "更新/归档候选",
        conflict: "冲突复核",
      },
      maintenanceActionOptions: {
        mark_reviewed: "已复核",
        snooze: "稍后处理",
        retrieval_reviewed: "检索调优已记录",
        update_candidate_created: "更新候选已生成",
        archived: "已归档",
      },
      maintenanceAutomatic: "后台自动维护",
      maintenanceNoUserAction: "无需操作；DS Agent 会在后台处理更新、归档和候选决策，这里只保留审计与纠错线索。",
      maintenanceSnoozeUntil: "稍后至",
      maintenanceLastAction: "最近维护",
      maintenanceQuality: "质量分",
      maintenanceQualitySignals: "质量信号",
      maintenanceRecommendedActions: "建议动作",
      maintenanceCreateUpdateCandidate: "生成更新候选",
      maintenanceCandidateCreated: "DS Agent 已根据反馈生成更新候选，并交由后台维护处理。",
      maintenanceRetrievalReviewed: "检索已调优",
      maintenanceMarkReviewed: "标记已复核",
      maintenanceSnooze: "稍后",
      maintenanceActionRecorded: "维护审计已记录。",
      maintenanceActionFailed: "维护审计记录失败。",
      maintenanceArchive: "确认归档",
      maintenanceArchiveDone: "记忆已归档。",
      maintenanceArchiveFailed: "记忆归档失败。",
    },
    memoryFeedback: {
      title: "选中记忆反馈",
      useful: "有用",
      irrelevant: "无关",
      stale: "过时",
      conflicting: "冲突",
      shouldUpdate: "应更新",
      recorded: "记忆反馈已记录。",
      recordedWithMaintenance: (
        updateCandidates,
        retrievalReviews,
        autoUpdates,
        autoArchives,
        candidateDecisions,
        mergeCandidates,
        autoMerges,
        modelRewrites,
      ) => {
        const updates = updateCandidates > 0 ? `，已生成 ${updateCandidates} 条更新候选` : "";
        const merges = mergeCandidates > 0 ? `，已生成 ${mergeCandidates} 条合并压缩候选` : "";
        const decisions =
          candidateDecisions > 0 ? `，已自动处理 ${candidateDecisions} 个候选决策` : "";
        const applied = autoUpdates > 0 ? `，已自动更新 ${autoUpdates} 条记忆` : "";
        const merged = autoMerges > 0 ? `，已自动合并 ${autoMerges} 组记忆` : "";
        const archived = autoArchives > 0 ? `，已自动归档 ${autoArchives} 条过时记忆` : "";
        const rewrites = modelRewrites > 0 ? `，模型辅助改写 ${modelRewrites} 条` : "";
        const retrieval =
          retrievalReviews > 0 ? `，已记录 ${retrievalReviews} 条检索调优` : "";
        return `记忆反馈已记录，后台维护已自动运行${updates}${merges}${decisions}${applied}${merged}${archived}${rewrites}${retrieval}。`;
      },
      recordFailed: "记忆反馈记录失败。",
      options: {
        useful: "有用",
        irrelevant: "无关",
        stale: "过时",
        conflicting: "冲突",
        should_update: "应更新",
      },
    },
    audit: {
      title: "权限预检",
      browser: "浏览器",
      emailSend: "发邮件",
      computerControl: "控电脑",
      empty: "暂无权限审计",
      loadFailed: "权限审计加载失败。",
      pending: "检查中",
    },
    capabilities: {
      title: "工具能力与权限闭环",
      request: "请求",
      requesting: "请求中",
      experimental: "实验",
      pendingTitle: "待处理审批",
      noPending: "暂无待审批请求",
      approve: "批准",
      reject: "拒绝",
      resolving: "处理中",
      loadFailed: "工具能力加载失败。",
      requestFailed: "权限请求失败。",
      resolveFailed: "审批处理失败。",
      auditTitle: "最近审计",
    },
    browserTool: {
      title: "浏览器工具",
      urlPlaceholder: "https://example.com/report",
      browse: "浏览",
      browsing: "浏览中",
      outputTitle: "工具输出",
      noOutput: "暂无工具输出",
      approvalRequest: "审批",
      pendingHint: "已创建待审批请求，批准后再次浏览即可执行。",
      failed: "浏览失败。",
    },
    browserSubmitTool: {
      title: "浏览器提交审批",
      urlPlaceholder: "目标表单网址",
      summaryPlaceholder: "提交动作说明",
      requestSubmit: "请求提交",
      requestingSubmit: "请求中",
      pendingHint: "已创建浏览器提交审批请求；当前版本不会提交表单。",
      blocked: "浏览器提交只留下权限复核记录，未提交任何表单。",
      failed: "浏览器提交请求失败。",
    },
    networkSearchTool: {
      title: "联网搜索审批",
      queryPlaceholder: "搜索关键词",
      scopePlaceholder: "来源范围，例如：公开网页",
      requestSearch: "请求搜索",
      requestingSearch: "请求中",
      pendingHint: "已创建联网搜索待审批请求；批准前不会访问网络。",
      completed: "联网搜索已执行，并记录了来源链接。",
      blocked: "联网搜索未完成。请查看最近工具输出中的失败原因。",
      failed: "联网搜索请求失败。",
      sourceModelRequiredTitle: "需要联网搜索来源模型",
      sourceModelRequiredBody:
        "当前开源 alpha 使用免费来源关联联网搜索选项执行真实搜索，请先选择来源模型以保留可审计链接；本地浏览器和聚合器预设目前共用同一本地搜索实现。",
      sourceModelPlaceholder: "选择免费联网来源",
      sourceModelMissing: "请先选择联网搜索来源模型。",
      routeNotEnabled: "当前联网搜索路线尚未启用真实搜索。",
    },
    fileTool: {
      title: "文件工具",
      pathPlaceholder: "输入你本机的文件路径",
      read: "读取",
      reading: "读取中",
      pendingHint: "已创建待审批请求，批准后再次读取即可执行。",
      failed: "读取失败。",
    },
    fileWriteTool: {
      title: "文件写入审批",
      pathPlaceholder: "目标文件路径",
      summaryPlaceholder: "写入或修改说明",
      contentPlaceholder: "写入内容",
      requestWrite: "请求写入",
      requestingWrite: "请求中",
      pendingHint: "已创建文件写入审批请求；批准后再次提交即可写入。",
      completed: "文件已写入本地工作区。",
      blocked: "文件写入未执行。",
      failed: "文件写入请求失败。",
    },
    folderTool: {
      title: "证据文件夹",
      pathPlaceholder: "输入你本机的证据文件夹路径",
      ingest: "导入",
      ingesting: "导入中",
      pendingHint: "已创建待审批请求，批准后再次导入即可执行。",
      failed: "证据文件夹导入失败。",
    },
    terminalTool: {
      title: "终端只读工具",
      commandLabel: "只读诊断命令",
      run: "运行",
      running: "运行中",
      pendingHint: "已创建待审批请求，批准后再次运行即可执行。",
      failed: "终端只读命令失败。",
      writeTitle: "终端写入审批",
      writeCommandLabel: "待审批写入命令",
      writePlaceholder: "例如：npm install",
      requestWrite: "请求",
      requestingWrite: "请求中",
      writePendingHint: "已创建终端写入审批请求；当前版本不会直接执行写命令。",
      writeBlocked: "终端写入只留下权限复核记录，未执行命令。",
      writeFailed: "终端写入请求失败。",
      options: {
        pwd: "当前目录",
        "git status --short": "Git 状态",
        "git diff --stat": "Git 变更统计",
        "git branch --show-current": "当前分支",
      },
    },
    computerTool: {
      title: "屏幕读取审批",
      capture: "检查屏幕",
      capturing: "检查中",
      pendingHint: "已创建屏幕截图待审批请求，批准后再次检查即可执行。",
      captured: "屏幕截图已保存为本地证据文件。",
      unavailable: "屏幕截图未完成；请检查系统截屏权限或显示器可用性。",
      failed: "屏幕截图请求失败。",
    },
    computerControlTool: {
      title: "电脑控制审批",
      unlockTitle: "桌面控制本地解锁",
      unlockChallengeLabel: "本机码",
      unlockTokenPlaceholder: "输入本机码",
      unlockControl: "解锁",
      unlockingControl: "解锁中",
      unlockReady: "本地控制已短时解锁。",
      unlockRequired: "执行前需要本机短时解锁。",
      unlockExpires: "有效至",
      unlockFailed: "桌面控制解锁失败。",
      targetPlaceholder: "目标窗口、页面或控件",
      actionPlaceholder: "click:120,340 或 hotkey:ctrl+shift+p",
      requestControl: "请求控制",
      requestingControl: "请求中",
      pendingHint: "已创建电脑控制审批请求；批准后可重试一次，执行结构化动作。",
      executed: "电脑控制已执行，并已保存权限复核记录。",
      blocked: "电脑控制未执行；请检查动作格式、系统权限或本地输入路线。",
      failed: "电脑控制请求失败。",
    },
    emailTool: {
      title: "邮件发送审批",
      toPlaceholder: "收件人",
      subjectPlaceholder: "主题",
      bodyPlaceholder: "正文",
      requestSend: "请求发送",
      requestingSend: "请求中",
      pendingHint: "已创建邮件发送审批请求；批准后可重试一次，当前版本不会直接发送邮件。",
      blocked: "邮件发送只留下权限复核记录，未发送邮件。",
      failed: "邮件发送请求失败。",
    },
    emailDraftTool: {
      title: "邮件草稿审批",
      toPlaceholder: "草稿收件人",
      subjectPlaceholder: "草稿主题",
      bodyPlaceholder: "草稿正文",
      requestDraft: "请求草稿",
      requestingDraft: "请求中",
      pendingHint: "已创建邮件草稿审批请求；当前版本不会创建真实邮箱草稿。",
      blocked: "邮件草稿只留下权限复核记录，未创建邮箱草稿。",
      failed: "邮件草稿请求失败。",
    },
    emailReadTool: {
      title: "邮件读取审批",
      mailboxPlaceholder: "邮箱或文件夹",
      queryPlaceholder: "读取条件或线索",
      requestRead: "请求读取",
      requestingRead: "请求中",
      pendingHint: "已创建邮件读取审批请求；当前版本不会读取真实邮箱。",
      blocked: "邮件读取只留下权限复核记录，未读取邮箱。",
      failed: "邮件读取请求失败。",
    },
    driveReadTool: {
      title: "本地文件夹读取",
      locationPlaceholder: "本地文件夹路径",
      queryPlaceholder: "文件名或内容关键词",
      requestRead: "请求读取",
      requestingRead: "请求中",
      pendingHint: "已创建本地文件夹读取审批请求；批准后会读取本地文件夹。",
      completed: "本地文件夹读取已完成，并记录了结果。",
      blocked: "本地文件夹读取未完成；请查看最近工具输出中的失败原因。",
      failed: "本地文件夹读取请求失败。",
    },
    driveWriteTool: {
      title: "工作包导出",
      locationPlaceholder: "本地导出文件夹路径",
      summaryPlaceholder: "导出工作包说明",
      requestWrite: "导出工作包",
      requestingWrite: "请求中",
      pendingHint: "已创建本地文件夹导出审批请求；批准后会导出当前工作包 JSON。",
      completed: "工作包已导出到本地文件夹。",
      blocked: "工作包导出未完成；请查看最近工具输出中的失败原因。",
      failed: "工作包导出请求失败。",
    },
    inspector: {
      title: "设置",
      largeModel: "大模型",
      model: "模型",
      access: "权限",
      thinking: "思考",
      scope: "范围",
      theme: "风格",
    },
  },
  en: {
    brandTagline: "DeepSeek local agent",
    navLabel: "Primary",
    nav: {
      workbench: "Chat",
      memory: "Memory",
      approvals: "Approvals",
      newChat: "New chat",
      conversations: "Chats",
      untitledConversation: "Untitled chat",
      plugins: "Plugins",
      settings: "Settings",
      contextCompressed: "Compressed",
      pin: "Pin",
      unpin: "Unpin",
      pinned: "Pinned",
      archive: "Archive",
      rename: "Rename",
      renameConversation: "Rename chat",
    },
    appUpdate: {
      update: "Update",
      checking: "Checking",
      downloading: "Download update",
      install: "Install update",
      installing: "Updating",
      downloadReady: (version) => `${version} downloaded. Click to install.`,
      approvalRequired: "Approve this update step in the right panel, then click update again.",
      downloadFailed: "Update download failed",
      installStarted: (version) => `Installing ${version} silently and restarting DS Agent`,
      installFailed: "Update failed to start",
    },
    settingsPanel: {
      title: "Settings",
      deepSeekApiKey: "DeepSeek API key",
      fallbackApiKey: "Fallback DeepSeek API key",
      apiKeyPlaceholder: "Enter the primary key for this window",
      apiKeyConfiguredPlaceholder: "•••••••••••••••• (configured)",
      apiKeyReady: "API key passed startup check",
      fallbackApiKeyPlaceholder: "Tried automatically if the primary key fails",
      soulProfile: "Soul",
      soulProfileOpen: "Open Soul settings",
      soulProfileClose: "Close",
      soulProfileModalTitle: "Soul Profile",
      soulProfileModalDescription:
        "Store only stable naming, tone, and collaboration preferences. Do not store passwords, keys, account identifiers, identity documents, or temporary task details.",
      soulProfileGuides: [
        {
          title: "User",
          lines: [
            "preferred_name: the name or short name DS Agent should remember.",
            "address_as: the default way to address you.",
            "language_preferences: default Chinese, English, or bilingual preference.",
            "default_response_tone: default tone such as concise, warm, or formal.",
            "default_response_length: default answer length.",
            "formatting_preferences: default formatting preferences.",
            "initiative_level: how proactively DS Agent should move work forward.",
          ],
        },
        {
          title: "DS Agent",
          lines: [
            "user_calls_ds_agent: what you usually call this tool.",
            "ds_agent_should_refer_to_itself_as: how DS Agent should refer to itself.",
            "relationship_boundary: the collaboration boundary to preserve.",
          ],
        },
        {
          title: "Stable Preferences",
          lines: [
            "workflow_preferences: long-term workflow habits.",
            "writing_preferences: stable writing and editing preferences.",
            "confirmation_preferences: what DS Agent should ask before doing.",
            "privacy_preferences: privacy and local-file handling preferences.",
          ],
        },
        {
          title: "Never Store",
          lines: [
            "This section is a do-not-store reminder for secrets, passwords, accounts, and sensitive identity data.",
            "Temporary tasks, one-off paths, live prices, and same-day state should not go into Soul Profile.",
          ],
        },
      ],
      soulProfilePlaceholder:
        "Edit memory/soul.md for names, DS Agent naming, and default response preferences.",
      soulProfileSave: "Save Soul Profile",
      soulProfileSaving: "Saving",
      soulProfileSaved: "Soul Profile saved.",
      soulProfileExists: "Local Soul Profile enabled",
      soulProfileTemplate: "Template shown; not saved yet",
      soulProfileSummary: "Runtime summary",
      soulProfileEmpty: "Soul Profile content is required.",
      soulProfileLoadFailed: "Soul Profile failed to load.",
      soulProfileSaveFailed: "Soul Profile failed to save.",
      workspaceDirectory: "Workspace directory",
      chooseWorkspace: "Choose folder",
      saveWorkspace: "Save folder",
      balance: "DeepSeek balance",
      queryBalance: "Read live balance",
      queryingBalance: "Reading",
      balanceAvailable: "Balance available",
      balanceUnavailable: "Balance unavailable",
      balanceNotQueried: "Balance not read yet",
      balanceFailed: "DeepSeek balance query failed.",
      balanceEmpty: "DeepSeek returned no balance details.",
      attribution: "DS Agent is created and maintained by Lee take.",
    },
    controls: {
      modelRoute: "Model type",
      largeModelProvider: "Large model",
      accessMode: "Access mode",
      thinkingLevel: "Thinking level",
      themeStyle: "Interface style",
      language: "Interface language",
      networkSearchSourceModel: "Web search source model",
    },
    largeModelOptions: {
      deepseek: "DeepSeek",
      chatgpt: "External chat model",
      codex: "Local bridge route",
      custom: "Custom model",
    },
    modelOptions: {
      auto: "DeepSeek Auto",
      flash: "Flash",
      pro: "Pro",
    },
    accessOptions: {
      ask_every_step: "Every step asks",
      ask_on_risk: "Ask on risk",
      limited_auto: "Limited auto",
      full_access: "Full access",
    },
    thinkingOptions: {
      auto: "Thinking auto",
      fast: "Fast",
      standard: "Standard",
      deep: "Deep",
    },
    scopeOptions: {
      workspace: "Workspace",
    },
    themeOptions: {
      ink: "Ink landscape",
      porcelain: "Blue porcelain",
    },
    networkSearchSourceOptions: {
      free_web_source: "Free web source model",
      free_local_browser: "Free local browser search (alpha)",
      free_source_aggregator: "Free source aggregator (alpha)",
    },
    runtimePlatformOptions: {
      windows: "Windows",
      macos: "macOS",
      other: "Other platform",
    },
    codexBridgeTransportOptions: {
      http: "Local HTTP bridge service",
      stdio: "Local service startup deferred",
    },
    backendOptions: {
      network_search: {
        deepseek: "DeepSeek search orchestration",
        native_large_model: "Native large-model web search",
        source_backed_model: "Source-linked web-search option",
      },
      network_search_execution: {
        permission_audit_only: "Permission review only",
        source_backed_adapter: "Source-linked route execution",
        native_bridge_contract: "Selected model web search",
      },
      network_search_evidence: {
        pending_user_confirmation: "Pending confirmation",
        source_links_required: "Source links required",
      },
      email: {
        architecture_only: "Architecture only",
      },
      drive: {
        local_folder_export_package: "Local folders and export packages",
      },
      computer_screenshot: {
        codex_style_screen_capture: "Legacy local-service screen route",
        codex_bridge_screen_capture: "Configured local-service screen route",
        local_windows_screen_capture: "Local Windows screen inspection",
        local_macos_screen_capture: "Local macOS screen inspection",
      },
      computer_control: {
        codex_style_input_control: "Legacy local-service mouse and keyboard route",
        codex_bridge_input_control: "Configured local-service mouse and keyboard route",
        local_windows_input_control: "Local Windows mouse and keyboard control",
        local_macos_input_control: "Local macOS mouse and keyboard control",
      },
    },
    backendLabels: {
      title: "Tool Route Strategy",
      largeModelProvider: "Large model",
      networkSearch: "Network search",
      networkSearchSupport: "Search support",
      networkSearchSourceModel: "Search source model",
      networkSearchRoute: "Search route",
      networkSearchExecution: "Search execution",
      networkSearchEvidence: "Evidence policy",
      networkRequests: "Network requests",
      deepSeekOrchestration: "DeepSeek orchestration",
      confirmationGate: "Confirmation gate",
      email: "Email",
      drive: "Drive",
      computerScreenshot: "Screen read",
      computerControl: "Computer control",
      deepSeekApi: "DeepSeek API",
      deepSeekChatApi: "DeepSeek Chat API",
      deepSeekTelemetry: "DeepSeek telemetry",
      apiBaseUrl: "API base",
      chatEndpoint: "Chat endpoint",
      deepSeekModels: "DeepSeek models",
      apiKeyEnv: "Key environment",
      apiKeyConfigured: "Configured",
      apiKeyMissing: "Missing",
      chatReady: "Ready",
      chatNotReady: "Not ready",
      enabled: "Enabled",
      disabled: "Disabled",
      confirmationRequired: "Required",
      confirmationNotRequired: "Not required",
      screenshotBackendStatus: "Screen route status",
      screenshotPermission: "Screen permission",
      controlBackendStatus: "Control route status",
      controlPermission: "Control permission",
      codexBridgeRuntime: "Local bridge service",
      backendAvailable: "Available",
      backendUnavailable: "Unavailable",
      approvalRequired: "Approval required",
      osPermissionRequired: "OS permission required",
      osPermissionNotRequired: "No explicit OS permission",
      bridgeRequired: "Local bridge required",
      bridgeNotRequired: "Local bridge not required",
      bridgeEndpointConfigured: "Local address configured",
      bridgeEndpointMissing: "Local address missing",
      bridgeConnected: "Connected",
      bridgeNotConnected: "Not connected",
      bridgeTransportMissing: "Route type missing",
      nativeSupported: "Native",
      sourceModelRequired: "Source model required",
      notSelected: "Not selected",
      noTelemetry: "No calls yet",
      cacheHit: "Cache hit",
      cacheMiss: "Cache miss",
      cacheDisabled: "Cache disabled",
      cacheEntries: "Cache entries",
      clearCache: "Clear cache",
      clearingCache: "Clearing",
      cacheCleared: (count) => `Cleared ${count} cache ${count === 1 ? "entry" : "entries"}.`,
      cacheClearFailed: "DeepSeek cache clear failed.",
      tokens: "tokens",
      cost: "Cost",
      runtimePlatform: "Runtime platform",
      macosPath: "macOS path",
    },
    capabilityFamilyOptions: {
      file: "Files",
      network: "Network",
      browser: "Browser",
      email: "Email",
      drive: "Drive",
      terminal: "Terminal",
      computer_use: "Computer Use",
      app_update: "App Update",
      skill: "Skill",
    },
    capabilityOptions: {
      file_read: "Read files",
      file_write: "Write files",
      network_search: "Network search",
      browser_browse: "Browse web",
      browser_submit: "Submit web",
      email_read: "Read email",
      email_draft: "Draft email",
      email_send: "Send email",
      connector_attachment_read: "Download connected attachment",
      connector_write: "Change connected account",
      drive_read: "Read drive",
      drive_write: "Write drive",
      terminal_read: "Read terminal",
      terminal_write: "Write terminal",
      computer_screenshot: "Screenshot",
      computer_control: "Control computer",
      app_update_check: "Check app update",
      app_update_download: "Download app update",
      app_update_install: "Install app update",
      skill_use: "Use declarative skill",
    },
    capabilitySummaries: {
      file_read: "Read approved local files in the workspace.",
      file_write: "Create or modify drafts and exported artifacts.",
      network_search: "Search public sources and preserve citations.",
      browser_browse: "Open and inspect web pages.",
      browser_submit: "Fill or submit web forms with approval.",
      email_read: "Read selected email threads for task evidence.",
      email_draft: "Prepare drafts without sending.",
      email_send: "Send approved outbound email.",
      connector_attachment_read: "Download one explicitly approved connected-account attachment into the managed workspace as untrusted evidence.",
      connector_write: "Send or change connected-account content after explicit approval.",
      drive_read: "Read selected local-folder text evidence.",
      drive_write: "Export work packages to a selected local folder.",
      terminal_read: "Run read-only diagnostics and collect output.",
      terminal_write: "Run commands that can change files or system state.",
      computer_screenshot: "Capture or inspect the visible desktop.",
      computer_control: "Use mouse and keyboard actions with per-step approval.",
      app_update_check: "Read trusted release metadata and compare installed versions.",
      app_update_download: "Download a trusted installer into the isolated update directory.",
      app_update_install: "Start a verified installer and restart after explicit approval.",
      skill_use: "Load an installed, enabled, hash-verified declarative skill entry.",
    },
    riskOptions: {
      low: "Low risk",
      medium: "Medium risk",
      high: "High risk",
      critical: "Critical risk",
    },
    decisionOptions: {
      allow: "Allow",
      ask: "Ask",
      deny: "Deny",
    },
    accessStatusOptions: {
      auto_approved: "Auto approved",
      pending_approval: "Pending",
      approved: "Approved",
      rejected: "Rejected",
      denied: "Blocked",
    },
    accessGrantOptions: {
      not_granted: "Not granted",
      reusable: "Reusable",
      one_shot_available: "One-shot available",
      one_shot_consumed: "Consumed",
    },
    invocationStatusOptions: {
      succeeded: "Succeeded",
      pending_approval: "Pending",
      failed: "Failed",
    },
    workbench: {
      stage: "0.1.0 preview",
      title: "DeepSeek Chat Workbench",
      summary:
        "Chat and review results in the center, manage skills, plugins, and settings on the left, and follow execution status on the right.",
    },
    chatWorkbench: {
      title: "DeepSeek Chat Workbench",
      assistantLabel: "DS Agent",
      userLabel: "You",
      assistantMessage: "Type a question or instruction. I will check context, permissions, and plugins first, then connect to DeepSeek.",
      taskTitlePlaceholder: "Give this task a short title",
      composerPlaceholder: "Enter a question, text, or instruction",
      starterPromptsLabel: "Try an office task",
      quickAsk: "Create a management briefing from my evidence folder.",
      quickDraft: "Turn these meeting notes into actions, owners, deadlines, and risks.",
      quickAnalyze: "Continue the previous project, first explain which memories you used.",
      saveTask: "Send",
      queueTask: "Queue new task",
      stopTask: "Stop",
      queueGuidance: "Add to current task",
      addAttachment: "Add files",
      removeAttachment: "Remove attachment",
      attachmentBlocked: "Blocked",
      attachmentMetadataOnly: "metadata only",
      attachmentDesktopOnly: "Add local files or images from the DS Agent desktop window.",
      attachmentAddFailed: "Attachment add failed. Choose the file again.",
      attachmentsOnlyPrompt: "Please use the attached files and images for this task.",
      guidanceQueued: "Supplement received; DS Agent will guide it after the current step",
      guidanceRunning: "Guiding the supplement into the same running task",
      guidanceQueuedFeedback: "Supplement received. DS Agent will fold it into the same task after the current step.",
      guidanceRunningFeedback: "Guiding the supplement now. DS Agent will consider it together with the running task.",
      taskQueuedFeedback: "Added to the background queue. It will run after the current task finishes.",
      stopRequestedFeedback: "Stop requested for the current task. This run's reply will not be appended.",
      readyStatus: "Input is preprocessed before DS Agent connects to DeepSeek",
      sendingStatus: "Preprocessing and connecting to DeepSeek",
      pendingStages: [
        "Preprocessing the request and checking local state",
        "Sent to DeepSeek and waiting for the structured reply",
        "DeepSeek is still generating; DS Agent will keep validating actions",
        "The request is still running; the desktop app has not frozen",
      ],
      loopGoalDetail: "Build the goal contract, done-when criteria, and boundaries",
      loopVerifyDetail: "Verify the real result against the user's goal",
      searchPending: "I created a network-search approval request. Confirm it to continue.",
      actionPlanLabel: "DS Agent pending actions",
      missingPrerequisitesLabel: "Needed setup",
      memoryCandidatesLabel: "Memory candidates queued for review",
      actionState: {
        proposed: "Pending",
        waiting_prerequisite: "Waiting for setup",
        needs_confirmation: "Needs confirmation",
        blocked: "Blocked",
        succeeded: "Done",
        failed: "Failed",
      },
      emptyPrompt: "Enter a question or instruction first.",
      desktopRuntimeMissing: "This is the browser preview, so the DS Agent desktop kernel is not connected. Run the Tauri desktop window before connecting to DeepSeek.",
      deepSeekRequestFailed: "The DeepSeek request failed. DS Agent did not run any local action. Please try again shortly.",
      deepSeekResponseReadFailed: "DeepSeek returned a response DS Agent could not read. No local action was run. Please try again shortly.",
      deepSeekKeyTitle: "Connect DeepSeek",
      deepSeekKeyBody: "Enter a DeepSeek API key. For now it is kept in this session only and is not written to source or local files.",
      deepSeekKeyPlaceholder: "sk-...",
      deepSeekKeyRequired: "Enter a DeepSeek API key first.",
      workspaceTitle: "Set Workspace",
      workspaceBody: "Choose one workspace. DS Agent will create and maintain the project structure inside it.",
      networkSearchTitle: "Configure Network Search",
      networkSearchBody: "This instruction appears to need web information. Choose a search source model to continue; key-backed search sources use their provider configuration.",
      approvalSummary: (count) =>
        `This task needs approval for the ${count} action${count === 1 ? "" : "s"} below. One confirmation runs them in order, or you can reject the whole task.`,
      taskApprovalRejected: "The user rejected the actions awaiting approval for this task.",
      confirmAndRun: "Confirm and run",
      confirmingAction: "Running",
      resumeAction: "Continue",
      resumingAction: "Running",
      resumeActionFailed: "Could not continue after approval.",
      continue: "Continue",
      cancel: "Cancel",
    },
    skills: {
      title: "Skills & Plugins",
      enabled: "Enabled",
      disabled: "Disabled",
      operationsTitle: "Operations Briefing",
      operationsDescription: "Reads local evidence and uses DeepSeek to produce a reviewable management brief.",
      autoInvoke: "DS Agent automatically uses installed skills and plugins when a task needs them.",
      systemSkills: "System Skills",
      installedSkills: "Installed Skills",
      installedPlugins: "Installed Plugins",
      scenarioTemplates: "Scenario Templates",
      protected: "System protected",
      automaticUpdates: "Checks and upgrades automatically in the background at startup",
      checkingUpdates: "Checking plugin updates in the background…",
      updateSummary: (checked, updated, failed) =>
        `Checked ${checked}, updated ${updated}, failed ${failed}.`,
      installFromChat: "Send a GitHub or Hugging Face URL in chat and DS Agent installs it automatically.",
      version: "Version",
      source: "Source",
      localSource: "Locally generated",
      skillType: "Skill",
      pluginType: "Plugin",
      systemSkillType: "System Skill",
      installedTitle: "Local Skill Registry",
      manifestPlaceholder:
        "Paste a ds-agent.skill.v1 manifest JSON. This version registers declarative skills only; scripts and binaries are not executed.",
      remotePackageUrlPlaceholder: "GitHub/Hugging Face Skill zip URL",
      installManifest: "Install manifest",
      installZip: "Import zip",
      previewRemote: "Preview remote",
      installRemote: "Install remote",
      installing: "Validating",
      empty: "No local skills installed yet.",
      enable: "Enable",
      disable: "Disable",
      verifySource: "Verify source",
      prepareExecution: "Prepare run",
      resetTrust: "Reset trust",
      uninstall: "Uninstall",
      executionsTitle: "Execution audit",
      safeBoundary:
        "DS Agent only validates and registers the manifest; scripts, binaries, and high-risk permissions are blocked by default.",
      noPermissions: "No permissions declared",
      installSucceeded: "Skill manifest passed validation and was registered.",
      previewSucceeded: (name: string, fileCount: number) =>
        `Remote package preflight passed: ${name}, ${fileCount} files.`,
      statusChanged: "Skill status recorded.",
      enabledNotice: "Enabled and available for automatic use.",
      disabledNotice: "Disabled and removed from automatic use.",
      sourceVerified: (provenance: string) => `Source verified: ${provenance}`,
      executionPrepared: (name: string) => `Safe execution plan prepared: ${name}`,
      executionBlocked: (name: string, reason: string) =>
        `Skill execution blocked: ${name} (${reason})`,
      trustReset: "Skill trust was reset and disabled until review.",
      uninstalled: "Skill uninstalled. Audit history was retained.",
      loadFailed: "Could not read the local skill registry.",
      installFailed: "Skill manifest did not pass safety validation.",
      previewFailed: "Remote skill package preflight failed.",
      statusFailed: "Could not update skill status.",
      sourceFailed: "Could not verify skill source.",
      executionFailed: "Could not prepare skill execution.",
      trustResetFailed: "Could not reset skill trust.",
      uninstallFailed: "Could not uninstall skill.",
      updateFailed: "Background automatic update check failed.",
    },
    runStatus: {
      title: "Run Status",
      current: "Current status",
      workflowSteps: "Workflow Steps",
      readyTitle: "Waiting for your instruction",
      readyBody: "Type a question or instruction in the center chat. When tools, permissions, or workflows are needed, this side panel shows execution status.",
      runningTitle: "Workflow running",
      runningBody: "DS Agent is reading evidence, assembling context, and calling DeepSeek.",
      needsApprovalTitle: "Waiting for your confirmation",
      needsApprovalBody: "A permission request needs review before the workflow can continue.",
      doneTitle: "Draft ready",
      doneBody: "The workflow produced reviewable results. Review and export them from the center workbench.",
      failedTitle: "Run needs attention",
      failedBody: "Check the tool record or error message, adjust the input, and retry.",
      agentActionTitle: "Agent actions parsed",
      agentActionBody: "DeepSeek returned a structured plan. DS Agent separated the reply from local action proposals.",
      permissionsAndTools: "Permissions and tools",
      routeDetails: "Model and route details",
      recentRuns: "Recent runs",
      recentTools: "Recent tool executions",
      queuedRuns: "Queued",
      runStepsLabel: (count: number) => `${count} steps`,
      runArtifactsLabel: (count: number) => `${count} artifacts`,
      workerLabel: (worker: string) => `Worker: ${worker}`,
      recoveryLabel: (count: number) => `Recovered ${count} time${count === 1 ? "" : "s"}`,
      agentRunStatus: {
        queued: "Queued",
        running: "Running",
        waiting_for_prerequisite: "Waiting for prerequisite",
        waiting_for_confirmation: "Awaiting confirmation",
        blocked: "Blocked",
        cancel_requested: "Cancelling",
        completed: "Completed",
        failed: "Failed",
        cancelled: "Cancelled",
      },
      toolStatus: {
        waiting_for_confirmation: "Awaiting approval",
        running: "Running",
        succeeded: "Verified",
        failed: "Failed",
        blocked: "Blocked",
      },
      steps: {
        understand: "Understand task",
        attachments: "Attachment evidence",
        evidence: "Read evidence",
        memory: "Select memory",
        deepseek: "Call DeepSeek",
        guidance: "Supplement",
        validate: "Validate result",
        report: "Generate and export",
      },
      stepDetails: {
        understand: "Waiting for a question or instruction in the chat.",
        attachments: (ready, metadataOnly, blocked) =>
          `Current task attachments: ${ready} ready, ${metadataOnly} metadata only, ${blocked} blocked.`,
        evidence: "Evidence is read or generated inside the workspace when needed.",
        memory: "Local memory will be selected when useful.",
        deepseek: "Waiting for DeepSeek Chat readiness.",
        validate: "Validation runs after generation.",
        report: "Export a report or work package after results are ready.",
      },
      stepState: {
        done: "Done",
        current: "Running",
        waiting: "Waiting",
        needs_action: "Needs action",
        blocked: "Blocked",
      },
    },
    localSetup: {
      title: "Workspace",
      required: "First run needs a local workspace.",
      ready: "Workspace is configured.",
      appData: "App data directory",
      settingsFile: "Directory settings",
      workspaceName: "Workspace name",
      workspaceNamePlaceholder: "e.g. Operations Analysis",
      workspaceDir: "Workspace",
      workspacePlaceholder: "Choose or enter a local workspace",
      choose: "Choose",
      chooseFailed: "Folder picker failed to open.",
      workspaceDialogTitle: "Choose workspace",
      save: "Save workspace",
      saving: "Saving",
      saved: "Workspace saved. DS Agent will maintain the project structure automatically.",
      failed: "Workspace setup failed.",
      loadFailed: "Workspace settings failed to load.",
      managedStructure:
        "DS Agent automatically manages evidence, exports, reports, runs, sources, work packages, memory, and logs inside the workspace.",
    },
    deepSeekPricing: {
      title: "DeepSeek Pricing",
      enabled: "Enable cost estimates",
      disabled: "Cost estimates disabled",
      statusConfigured: "Local pricing configured",
      statusNotConfigured: "Local pricing not configured",
      help: "Enter USD / 1M tokens. Prices are stored in local app data, not hardcoded into the open-source project.",
      settingsFile: "Pricing settings",
      flashPrompt: "Flash input",
      flashCompletion: "Flash output",
      proPrompt: "Pro input",
      proCompletion: "Pro output",
      pricePlaceholder: "e.g. 0.14",
      save: "Save pricing",
      saving: "Saving",
      saved: "DeepSeek pricing saved.",
      failed: "DeepSeek pricing save failed.",
      loadFailed: "DeepSeek pricing failed to load.",
    },
    operationsBriefing: {
      title: "Operations Briefing Workflow",
      run: "Run",
      running: "Running",
      seedTemplates: "Seed templates",
      seededTemplates: "Blank operator templates seeded into the local evidence folder.",
      seedPendingHint: "A file write approval request was created. Approve it, then seed again.",
      seedFailed: "Evidence template seeding failed.",
      exportPackage: "Export brief package",
      exportReport: "Export report",
      exportHtmlReport: "Export HTML",
      exportPdfReport: "Export PDF",
      exported: "Briefing work package generated.",
      reportExported: "Operations briefing Markdown report exported to the local export folder.",
      reportPendingHint:
        "A local folder export approval request was created. Approve it, then export again.",
      reportExportFailed: "Operations briefing report export failed.",
      htmlReportExported: "Operations briefing HTML report exported to the local export folder.",
      htmlReportPendingHint:
        "A local folder export approval request was created. Approve it, then export HTML again.",
      htmlReportExportFailed: "Operations briefing HTML report export failed.",
      pdfReportExported: "Operations briefing PDF report exported to the local export folder.",
      pdfReportPendingHint:
        "A local folder export approval request was created. Approve it, then export PDF again.",
      pdfReportExportFailed: "Operations briefing PDF report export failed.",
      latestRun: "Latest Run",
      runs: "Run Archive",
      noRuns: "No operations briefing runs yet",
      pendingHint: "A pending approval request was created. Approve it, then run again.",
      failed: "Operations briefing run failed.",
      loadFailed: "Operations briefing runs failed to load.",
      anomalies: "Anomaly Leads",
      actions: "Actions",
      noAnomalies: "No anomaly leads",
      noActions: "No actions",
      evidence: "Evidence",
      archived: "Archived",
      archiveEvidenceRedacted: "Source evidence handle redacted",
      contextReceipt: "Context Receipt",
      contextUserIntent: "User intent",
      contextLoopMode: "Loop mode",
      contextWorkflowPolicy: "Workflow policy",
      contextSelectedEvidence: "Selected evidence",
      contextSelectedMemories: "Selected memories",
      contextMemoryRetrieval: "Memory retrieval",
      contextMemoryScores: "Memory scores",
      contextMemoryConflictHints: "Memory hints",
      contextMemoryCandidateGate: "Memory candidate gate",
      contextNoSelectedMemories: "No memory selected for this run",
      contextModelRoute: "Model route",
      contextThinkingLevel: "Thinking level",
      contextTokenCache: "Token / cache",
      contextValidation: "Validation",
      contextIntentionalOmissions: "Intentional omissions",
      contextNoItems: "None",
      status: {
        pending_approval: "Pending",
        draft_ready: "Draft ready",
        failed: "Failed",
      },
    },
    package: {
      title: "Task Records and Work Packages",
      taskTitle: "Task title",
      taskSummary: "Task summary",
      addRecord: "Add record",
      exportPackage: "Export package",
      copyPackage: "Copy",
      importPackage: "Import",
      previewImport: "Preview",
      previewing: "Previewing",
      previewTitle: "Import Preview",
      previewReady: "Work package preview generated.",
      previewFailed: "Work package preview failed.",
      previewTotalTasks: "Total tasks",
      previewNewTasks: "New tasks",
      previewSkippedTasks: "Skipped tasks",
      previewMemoryCandidates: "Memory candidates",
      previewNewMemoryCandidates: "New memory candidates",
      previewSkippedMemoryCandidates: "Skipped memory candidates",
      previewMemoryCandidateHint:
        "Memory candidates import into local review and do not write long-term memory.",
      previewMemoryCandidateReviewSupported: "Memory candidates can enter local review.",
      previewMemoryCandidateReviewUnsupported:
        "This version cannot review these memory candidates.",
      previewArchivedRuns: "Brief archives",
      previewNewArchivedRuns: "New brief archives",
      previewSkippedArchivedRuns: "Skipped brief archives",
      previewArchiveHint:
        "Briefing runs import as read-only archives and do not rerun tools.",
      previewArchiveReplaySupported: "Brief archives can be replayed locally in read-only mode.",
      previewArchiveReplayUnsupported: "This version cannot replay these brief archives.",
      previewWorkflowTemplates: "Workflow templates",
      previewNewWorkflowTemplates: "New templates",
      previewSkippedWorkflowTemplates: "Skipped templates",
      previewWorkflowTemplateHint:
        "Workflow template packages import as local available assets and do not write user folders.",
      previewWorkflowTemplateImportSupported:
        "Workflow templates can be registered as local available assets.",
      previewWorkflowTemplateImportUnsupported:
        "This version cannot import these workflow templates.",
      packageJson: "Work package JSON",
      importJson: "Import JSON",
      emptyTitle: "Add a task title first.",
      emptyImport: "Paste work package JSON first.",
      created: "Task record saved to the local event store.",
      exported: "Work package generated.",
      copied: "Work package JSON copied.",
      imported: (
        imported,
        skipped,
        candidateImported,
        candidateSkipped,
        briefingImported,
        briefingSkipped,
        templateImported,
        templateSkipped,
      ) =>
        `Import complete: ${imported} tasks added, ${skipped} skipped; ${candidateImported} memory candidates added, ${candidateSkipped} skipped; ${briefingImported} brief archives added, ${briefingSkipped} skipped; ${templateImported} workflow templates added, ${templateSkipped} skipped.`,
      noRecords: "No task records yet",
      copyFailed: "Copy failed. Select the JSON manually.",
      loadFailed: "Task records failed to load.",
    },
    memory: {
      title: "Auto Memory",
      autoCapture: "Captured from task records",
      noMemories: "No auto memories yet",
      loadFailed: "Memories failed to load.",
      search: "Search",
      searchPlaceholder: "Search memories",
      candidateTitle: "Memory candidate title",
      candidateBody: "Memory candidate body",
      candidateType: "Type",
      candidateScope: "Scope",
      candidateSensitivity: "Sensitivity",
      candidateLifecycle: "Lifecycle",
      expiresAt: "Expiration date",
      metadata: "Memory metadata",
      propose: "Propose memory",
      proposing: "Proposing",
      candidates: "Memory Candidates",
      noCandidates: "No memory candidates",
      accept: "Background accept",
      reject: "Background reject",
      edit: "Edit",
      editTitle: "Memory title",
      editBody: "Memory body",
      save: "Save",
      saving: "Saving",
      cancel: "Cancel",
      delete: "Delete",
      deleting: "Deleting",
      resolving: "Resolving",
      proposed: "Memory candidate submitted for review.",
      accepted: "Memory candidate saved to long-term memory by background maintenance.",
      rejected: "Memory candidate rejected.",
      updated: "Long-term memory updated.",
      deleted: "Long-term memory deleted.",
      emptyCandidate: "Add a memory candidate title and body first.",
      emptyEdit: "Add a memory title and body first.",
      emptyExpiration: "Choose an expiration date.",
      proposeFailed: "Memory candidate proposal failed.",
      resolveFailed: "Memory candidate update failed.",
      updateFailed: "Long-term memory update failed.",
      deleteFailed: "Long-term memory delete failed.",
      conflictWarning: (count) =>
        `May overlap with ${count} long-term ${count === 1 ? "memory" : "memories"}`,
      conflictDetails: "Overlapping memories",
      previewMerge: "Preview merge",
      previewingMerge: "Previewing",
      mergePreviewTitle: "Merge draft",
      mergePreviewDraft: "Draft body",
      mergePreviewFailed: "Memory merge preview failed.",
      mergeAndAccept: "Background merge",
      merged: "Memory candidate merged and older memories archived.",
      mergeFailed: "Memory candidate merge failed.",
      previewReplace: "Preview replace",
      previewingReplace: "Previewing",
      replacePreviewTitle: "Replace preview",
      replacePreviewDraft: "Replacement draft",
      replacePreviewTargets: "Would replace",
      replacePreviewFailed: "Memory replace preview failed.",
      replaceAndAccept: "Background replace",
      replaced: "Background maintenance saved the replacement and archived older memories.",
      replaceFailed: "Memory candidate replace failed.",
      updateAndAccept: "Update existing",
      updatedFromCandidate: "Background maintenance updated the existing memory.",
      updateCandidateFailed: "Memory candidate update failed.",
      archiveStaleTarget: "Archive stale target",
      archivedFromCandidate: "Background maintenance archived the stale memory.",
      archiveCandidateFailed: "Memory candidate archive failed.",
      linkAndAccept: "Background link",
      linkRelation: "Link relation",
      linkExisting: "Link existing memories",
      linkingExisting: "Linking",
      linkSource: "Source memory",
      linkTarget: "Target memory",
      linkExistingNote: "Why these memories are linked",
      emptyExistingLink: "Choose two different long-term memories.",
      existingLinked: "Long-term memories linked.",
      existingLinkFailed: "Long-term memory link failed.",
      linked: "Background maintenance linked the candidate to overlapping memories.",
      linkFailed: "Memory candidate link failed.",
      linkedMemories: (count) => `Linked to ${count} ${count === 1 ? "memory" : "memories"}`,
      linkNote: "Link note",
      searchMatchedBy: "Matched by",
      searchMatchUnknown: "linked memory",
      searchMatchOptions: {
        direct: "Direct match",
        linked_memory_title: "Linked title",
        linked_memory_body: "Linked body",
      },
      updatedAt: "Updated",
      candidateGate: "Candidate gate",
      candidatePrivacyReview: "Privacy review",
      candidateSuggestedAction: "Suggested action",
      candidateEvidenceExcerpt: "Evidence excerpt",
      candidateSuggestedActionOptions: {
        new: "New",
        update: "Update",
        merge: "Merge",
        replace: "Replace",
        archive: "Archive",
        link: "Link",
        reject_hint: "Reject hint",
      },
      candidateStatus: {
        pending: "Pending",
        accepted: "Accepted",
        rejected: "Rejected",
      },
      typeOptions: {
        preference: "Preference",
        project_context: "Project context",
        workflow_rule: "Workflow rule",
        artifact: "Artifact",
        failure_pattern: "Failure pattern",
      },
      scopeOptions: {
        workspace: "Workspace",
        project: "Project",
        organization: "Organization",
        user: "User",
      },
      sensitivityOptions: {
        normal: "Normal",
        sensitive: "Sensitive",
      },
      lifecycleOptions: {
        active: "Active",
        archived: "Archived",
        expires: "Expires",
      },
      relationOptions: {
        related: "Related",
        updates: "Updates",
        extends: "Extends",
        derives: "Derives",
      },
      feedbackReview: "Feedback review",
      feedbackReviewEmpty: "No recorded memory feedback yet",
      feedbackReviewCount: (count) => `${count} feedback ${count === 1 ? "entry" : "entries"}`,
      feedbackFilter: "Feedback filter",
      feedbackSort: "Feedback sort",
      feedbackFilterOptions: {
        all: "All feedback",
        needs_review: "Needs review",
        useful: "Useful",
        irrelevant: "Irrelevant",
        stale: "Stale",
        conflicting: "Conflicting",
        should_update: "Should update",
      },
      feedbackSortOptions: {
        priority: "Priority",
        latest: "Latest feedback",
        feedback_count: "Feedback count",
      },
      latestFeedback: "Latest feedback",
      needsFeedbackReview: "Needs review",
      feedbackMemoryMissing: "This memory is not currently visible",
      maintenanceReview: "Background maintenance audit",
      maintenanceReviewEmpty: "No background maintenance records to show",
      maintenanceReviewCount: (count) =>
        `${count} maintenance ${count === 1 ? "signal" : "signals"}`,
      maintenanceFilter: "Maintenance filter",
      maintenanceSort: "Maintenance sort",
      maintenanceFilterOptions: {
        all: "All maintenance",
        needs_review: "Needs confirmation",
        retrieval: "Retrieval tuning",
        update_archive: "Update/archive",
        conflict: "Conflict",
        snoozed: "Snoozed",
      },
      maintenanceSortOptions: {
        priority: "Priority",
        latest: "Latest feedback",
        feedback_count: "Feedback count",
      },
      maintenanceReviewKindOptions: {
        retrieval: "Retrieval tuning",
        update_archive: "Update/archive candidate",
        conflict: "Conflict review",
      },
      maintenanceActionOptions: {
        mark_reviewed: "Reviewed",
        snooze: "Snoozed",
        retrieval_reviewed: "Retrieval tuning recorded",
        update_candidate_created: "Update candidate created",
        archived: "Archived",
      },
      maintenanceAutomatic: "Background maintenance",
      maintenanceNoUserAction:
        "No action needed; DS Agent handles updates, archives, and candidate decisions in the background while this view keeps the audit and correction trail.",
      maintenanceSnoozeUntil: "Snoozed until",
      maintenanceLastAction: "Latest maintenance",
      maintenanceQuality: "Quality score",
      maintenanceQualitySignals: "Quality signals",
      maintenanceRecommendedActions: "Recommended actions",
      maintenanceCreateUpdateCandidate: "Create update candidate",
      maintenanceCandidateCreated:
        "DS Agent created an update candidate from feedback and handed it to background maintenance.",
      maintenanceRetrievalReviewed: "Retrieval tuned",
      maintenanceMarkReviewed: "Mark reviewed",
      maintenanceSnooze: "Snooze",
      maintenanceActionRecorded: "Maintenance audit recorded.",
      maintenanceActionFailed: "Maintenance audit failed to record.",
      maintenanceArchive: "Confirm archive",
      maintenanceArchiveDone: "Memory archived.",
      maintenanceArchiveFailed: "Memory archive failed.",
    },
    memoryFeedback: {
      title: "Selected memory feedback",
      useful: "Useful",
      irrelevant: "Irrelevant",
      stale: "Stale",
      conflicting: "Conflicting",
      shouldUpdate: "Should update",
      recorded: "Memory feedback recorded.",
      recordedWithMaintenance: (
        updateCandidates,
        retrievalReviews,
        autoUpdates,
        autoArchives,
        candidateDecisions,
        mergeCandidates,
        autoMerges,
        modelRewrites,
      ) => {
        const updates =
          updateCandidates > 0 ? `, created ${updateCandidates} update candidate(s)` : "";
        const merges =
          mergeCandidates > 0 ? `, created ${mergeCandidates} merge/compression candidate(s)` : "";
        const decisions =
          candidateDecisions > 0
            ? `, resolved ${candidateDecisions} candidate decision(s)`
            : "";
        const applied =
          autoUpdates > 0 ? `, automatically updated ${autoUpdates} memory record(s)` : "";
        const merged =
          autoMerges > 0 ? `, automatically merged ${autoMerges} memory group(s)` : "";
        const archived =
          autoArchives > 0 ? `, automatically archived ${autoArchives} stale record(s)` : "";
        const rewrites =
          modelRewrites > 0 ? `, used model rewrites for ${modelRewrites} memory record(s)` : "";
        const retrieval =
          retrievalReviews > 0 ? `, recorded ${retrievalReviews} retrieval tuning item(s)` : "";
        return `Memory feedback recorded and background maintenance ran${updates}${merges}${decisions}${applied}${merged}${archived}${rewrites}${retrieval}.`;
      },
      recordFailed: "Memory feedback failed to record.",
      options: {
        useful: "Useful",
        irrelevant: "Irrelevant",
        stale: "Stale",
        conflicting: "Conflicting",
        should_update: "Should update",
      },
    },
    audit: {
      title: "Permission Check",
      browser: "Browser",
      emailSend: "Email",
      computerControl: "Computer",
      empty: "No permission audits yet",
      loadFailed: "Permission audits failed to load.",
      pending: "Checking",
    },
    capabilities: {
      title: "Tools and Permission Loop",
      request: "Request",
      requesting: "Requesting",
      experimental: "Experimental",
      pendingTitle: "Pending Approvals",
      noPending: "No pending approval requests",
      approve: "Approve",
      reject: "Reject",
      resolving: "Resolving",
      loadFailed: "Tool capabilities failed to load.",
      requestFailed: "Permission request failed.",
      resolveFailed: "Approval update failed.",
      auditTitle: "Recent Audit",
    },
    browserTool: {
      title: "Browser Tool",
      urlPlaceholder: "https://example.com/report",
      browse: "Browse",
      browsing: "Browsing",
      outputTitle: "Tool Output",
      noOutput: "No tool output yet",
      approvalRequest: "Approval",
      pendingHint: "A pending approval request was created. Approve it, then browse again.",
      failed: "Browse failed.",
    },
    browserSubmitTool: {
      title: "Browser form submission approval",
      urlPlaceholder: "Target form URL",
      summaryPlaceholder: "Submission action summary",
      requestSubmit: "Request submit",
      requestingSubmit: "Requesting",
      pendingHint:
        "A browser form submission approval request was created. This preview will not submit any form.",
      blocked:
        "Browser form submission keeps a permission review record. No form was submitted.",
      failed: "Browser form submission request failed.",
    },
    networkSearchTool: {
      title: "Web search approval",
      queryPlaceholder: "Search query",
      scopePlaceholder: "Source scope, for example: public web",
      requestSearch: "Request search",
      requestingSearch: "Requesting",
      pendingHint:
        "A web search approval request was created. It will not access the network before approval.",
      completed: "Web search ran and recorded source links.",
      blocked:
        "Web search did not complete. Check recent tool output for the failure reason.",
      failed: "Web search request failed.",
      sourceModelRequiredTitle: "Web search source model required",
      sourceModelRequiredBody:
        "This open-source alpha uses source-linked web search for live results. Choose a free source-linked web-search option first so auditable links are preserved; some early options currently share the same local search implementation.",
      sourceModelPlaceholder: "Choose a free source-linked web-search option",
      sourceModelMissing: "Choose a web search source model first.",
      routeNotEnabled: "The current web search route is not enabled for live search.",
    },
    fileTool: {
      title: "File Tool",
      pathPlaceholder: "Enter a local file path",
      read: "Read",
      reading: "Reading",
      pendingHint: "A pending approval request was created. Approve it, then read again.",
      failed: "File read failed.",
    },
    fileWriteTool: {
      title: "File write approval",
      pathPlaceholder: "Target file path",
      summaryPlaceholder: "Write or change summary",
      contentPlaceholder: "File content",
      requestWrite: "Request write",
      requestingWrite: "Requesting",
      pendingHint: "A file write approval request was created. Approve it, then submit again.",
      completed: "File written to the local workspace.",
      blocked: "File write did not execute.",
      failed: "File write request failed.",
    },
    folderTool: {
      title: "Evidence Folder",
      pathPlaceholder: "Enter a local evidence folder path",
      ingest: "Ingest",
      ingesting: "Ingesting",
      pendingHint: "A pending approval request was created. Approve it, then ingest again.",
      failed: "Evidence folder ingest failed.",
    },
    terminalTool: {
      title: "Terminal Read Tool",
      commandLabel: "Read-only diagnostic command",
      run: "Run",
      running: "Running",
      pendingHint: "A pending approval request was created. Approve it, then run again.",
      failed: "Terminal read command failed.",
      writeTitle: "Terminal write approval",
      writeCommandLabel: "Write command for approval",
      writePlaceholder: "Example: npm install",
      requestWrite: "Request",
      requestingWrite: "Requesting",
      writePendingHint:
        "A terminal write approval request was created. This preview will not execute write commands directly.",
      writeBlocked:
        "Terminal write keeps a permission review record. No command was run.",
      writeFailed: "Terminal write request failed.",
      options: {
        pwd: "Current directory",
        "git status --short": "Git status",
        "git diff --stat": "Git change stats",
        "git branch --show-current": "Current branch",
      },
    },
    computerTool: {
      title: "Screen inspection approval",
      capture: "Inspect screen",
      capturing: "Inspecting",
      pendingHint:
        "A screenshot approval request was created. Approve it, then inspect again.",
      captured: "Screenshot saved as local evidence.",
      unavailable:
        "Screenshot was not captured. Check OS screen-capture permission or display availability.",
      failed: "Screenshot request failed.",
    },
    computerControlTool: {
      title: "Computer control approval",
      unlockTitle: "Local computer control unlock",
      unlockChallengeLabel: "Local code",
      unlockTokenPlaceholder: "Enter local code",
      unlockControl: "Unlock",
      unlockingControl: "Unlocking",
      unlockReady: "Local control is unlocked for a short window.",
      unlockRequired: "Local short-window unlock is required before execution.",
      unlockExpires: "Valid until",
      unlockFailed: "Computer control unlock failed.",
      targetPlaceholder: "Target window, page, or control",
      actionPlaceholder: "click:120,340 or hotkey:ctrl+shift+p",
      requestControl: "Request control",
      requestingControl: "Requesting",
      pendingHint:
        "A computer control approval request was created. After approval, retry once to execute the structured action.",
      executed: "Computer control executed and saved a permission review record.",
      blocked:
        "Computer control was not executed. Check action format, OS permission, or the local input route.",
      failed: "Computer control request failed.",
    },
    emailTool: {
      title: "Email send approval",
      toPlaceholder: "Recipient",
      subjectPlaceholder: "Subject",
      bodyPlaceholder: "Body",
      requestSend: "Request send",
      requestingSend: "Requesting",
      pendingHint:
        "An email send approval request was created. After approval, retry once; this preview will not send email directly.",
      blocked: "Email send keeps a permission review record. No email was sent.",
      failed: "Email send request failed.",
    },
    emailDraftTool: {
      title: "Email draft approval",
      toPlaceholder: "Draft recipient",
      subjectPlaceholder: "Draft subject",
      bodyPlaceholder: "Draft body",
      requestDraft: "Request draft",
      requestingDraft: "Requesting",
      pendingHint:
        "An email draft approval request was created. This preview will not create mailbox drafts.",
      blocked:
        "Email draft keeps a permission review record. No mailbox draft was created.",
      failed: "Email draft request failed.",
    },
    emailReadTool: {
      title: "Email read approval",
      mailboxPlaceholder: "Mailbox or folder",
      queryPlaceholder: "Read query or evidence clue",
      requestRead: "Request read",
      requestingRead: "Requesting",
      pendingHint:
        "An email read approval request was created. This preview will not read a real mailbox.",
      blocked: "Email read keeps a permission review record. No mailbox was read.",
      failed: "Email read request failed.",
    },
    driveReadTool: {
      title: "Local folder read",
      locationPlaceholder: "Local folder path",
      queryPlaceholder: "File name or content keyword",
      requestRead: "Request read",
      requestingRead: "Requesting",
      pendingHint:
        "A local folder read approval request was created. After approval, it will read the selected local folder.",
      completed: "Local folder read completed and recorded the result.",
      blocked: "Local folder read did not complete. Check recent tool output for the failure reason.",
      failed: "Local folder read request failed.",
    },
    driveWriteTool: {
      title: "Work package export",
      locationPlaceholder: "Target local export folder",
      summaryPlaceholder: "Export package summary",
      requestWrite: "Export package",
      requestingWrite: "Requesting",
      pendingHint:
        "A local folder export approval request was created. After approval, it will export the current work package JSON.",
      completed: "Work package exported to the local folder.",
      blocked: "Work package export did not complete. Check recent tool output for the failure reason.",
      failed: "Work package export request failed.",
    },
    inspector: {
      title: "Settings",
      largeModel: "Large model",
      model: "Model",
      access: "Access",
      thinking: "Thinking",
      scope: "Scope",
      theme: "Style",
    },
  },
};
